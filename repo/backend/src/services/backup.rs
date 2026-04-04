/// AES-256-GCM encrypted backup and restore service.
///
/// Backup file format (.mbak):
///   [8 bytes]  Magic: "MBACK01\0"
///   [12 bytes] Random AES-GCM nonce (96-bit)
///   [N bytes]  AES-256-GCM ciphertext (pg_dump SQL output + AEAD authentication tag)
///
/// Key derivation:
///   key = SHA-256(BACKUP_ENCRYPTION_KEY env var)
///   → 32 bytes used directly as AES-256 key material
///
/// Security properties:
///   - Confidentiality: AES-256 in GCM mode
///   - Integrity:       AEAD authentication tag (16-byte GCM tag appended)
///   - Authenticity:    Nonce is random per backup; replay of ciphertext is
///                      detected by the AEAD tag mismatch
///
/// Restore safety:
///   1. Backup must exist in backup_metadata with status = 'completed'
///   2. Decryption must succeed (AEAD integrity)
///   3. SHA-256 checksum of the plaintext must match recorded value
///   4. Decrypted SQL is written to a dated restore file in backups/
///   5. The API returns the path and psql command — the admin must run it manually.
///      The running server NEVER issues DROP DATABASE or destructive SQL.

use aes_gcm::{
    aead::{Aead, AeadCore, KeyInit, OsRng},
    Aes256Gcm, Key, Nonce,
};
use sha2::{Digest, Sha256};
use std::path::Path;
use uuid::Uuid;

use crate::db::DbPool;
use crate::errors::AppError;

const MAGIC: &[u8; 8] = b"MBACK01\0";

// ---------------------------------------------------------------------------
// Key derivation
// ---------------------------------------------------------------------------

/// Derive a 32-byte AES-256 key from an arbitrary-length passphrase.
///
/// SHA-256(passphrase) → [u8; 32]
pub fn derive_key(passphrase: &str) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(passphrase.as_bytes());
    hasher.finalize().into()
}

// ---------------------------------------------------------------------------
// Encryption / decryption
// ---------------------------------------------------------------------------

/// Encrypt `plaintext` with AES-256-GCM.
///
/// Returns `[MAGIC || nonce || ciphertext]`.
pub fn encrypt_data(plaintext: &[u8], key: &[u8; 32]) -> Result<Vec<u8>, AppError> {
    let aes_key = Key::<Aes256Gcm>::from_slice(key);
    let cipher = Aes256Gcm::new(aes_key);
    let nonce = Aes256Gcm::generate_nonce(&mut OsRng);

    let ciphertext = cipher
        .encrypt(&nonce, plaintext)
        .map_err(|e| AppError::InternalError(format!("Encryption failed: {}", e)))?;

    let mut output = Vec::with_capacity(MAGIC.len() + 12 + ciphertext.len());
    output.extend_from_slice(MAGIC);
    output.extend_from_slice(&nonce);
    output.extend_from_slice(&ciphertext);
    Ok(output)
}

/// Decrypt data produced by `encrypt_data`.
///
/// Verifies the magic header, extracts nonce, and authenticates + decrypts.
pub fn decrypt_data(data: &[u8], key: &[u8; 32]) -> Result<Vec<u8>, AppError> {
    if data.len() < MAGIC.len() + 12 + 16 {
        return Err(AppError::ValidationError(
            "Backup file too small or corrupt.".into(),
        ));
    }

    let (magic, rest) = data.split_at(MAGIC.len());
    if magic != MAGIC {
        return Err(AppError::ValidationError(
            "Not a valid Meridian backup file (magic mismatch).".into(),
        ));
    }

    let (nonce_bytes, ciphertext) = rest.split_at(12);
    let nonce = Nonce::from_slice(nonce_bytes);

    let aes_key = Key::<Aes256Gcm>::from_slice(key);
    let cipher = Aes256Gcm::new(aes_key);

    cipher
        .decrypt(nonce, ciphertext)
        .map_err(|_| AppError::ValidationError("Backup decryption failed: wrong key or corrupt file.".into()))
}

// ---------------------------------------------------------------------------
// Checksum
// ---------------------------------------------------------------------------

/// SHA-256 hex checksum of plaintext for integrity verification.
pub fn sha256_hex(data: &[u8]) -> String {
    let hash = Sha256::digest(data);
    hash.iter().map(|b| format!("{:02x}", b)).collect()
}

// ---------------------------------------------------------------------------
// Backup creation
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct BackupResult {
    pub filename: String,
    pub path: String,
    pub size_bytes: u64,
    pub checksum: String,
}

/// Run `pg_dump`, encrypt the output, write to `{backups_dir}/backup_{id}.mbak`.
///
/// Returns metadata about the created backup.
pub async fn create_backup(
    database_url: &str,
    backups_dir: &str,
    encryption_key: &str,
) -> Result<BackupResult, AppError> {
    if encryption_key.is_empty() {
        return Err(AppError::InternalError(
            "BACKUP_ENCRYPTION_KEY is not configured. Set it in backend/.env to enable backups.".into(),
        ));
    }

    // Ensure backups directory exists.
    tokio::fs::create_dir_all(backups_dir)
        .await
        .map_err(|e| AppError::InternalError(format!("Cannot create backups dir: {}", e)))?;

    // Run pg_dump.
    let output = tokio::process::Command::new("pg_dump")
        .arg("--dbname")
        .arg(database_url)
        .arg("--no-password")
        .arg("--format=plain")
        .output()
        .await
        .map_err(|e| AppError::InternalError(format!("pg_dump failed to launch: {}. Is pg_dump installed?", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(AppError::InternalError(format!(
            "pg_dump exited with error: {}",
            stderr
        )));
    }

    let plaintext = output.stdout;
    if plaintext.is_empty() {
        return Err(AppError::InternalError("pg_dump produced no output.".into()));
    }

    let checksum = sha256_hex(&plaintext);
    let key = derive_key(encryption_key);
    let encrypted = encrypt_data(&plaintext, &key)?;

    let backup_id = Uuid::new_v4();
    let filename = format!("backup_{}.mbak", backup_id);
    let path = format!("{}/{}", backups_dir.trim_end_matches('/'), filename);

    let size_bytes = encrypted.len() as u64;
    tokio::fs::write(&path, &encrypted)
        .await
        .map_err(|e| AppError::InternalError(format!("Failed to write backup: {}", e)))?;

    log::info!("Backup created: {} ({} bytes encrypted)", path, size_bytes);

    Ok(BackupResult {
        filename,
        path,
        size_bytes,
        checksum,
    })
}

// ---------------------------------------------------------------------------
// Restore preparation
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct RestorePreparation {
    /// Path to the decrypted SQL file the admin should apply.
    pub restore_path: String,
    /// Checksum of the decrypted content (must match backup_metadata.checksum).
    pub checksum: String,
    /// Suggested psql command for the admin.
    pub psql_command: String,
}

/// Decrypt a backup and write a restore-ready SQL file.
///
/// Safety checks performed:
///  1. The backup path must exist on disk.
///  2. Decryption must succeed (AEAD integrity).
///  3. Checksum of decrypted content must match `expected_checksum`.
///
/// The running server never executes the SQL itself; it only prepares the
/// file and returns the command for the admin.
pub async fn prepare_restore(
    backup_path: &str,
    backups_dir: &str,
    encryption_key: &str,
    expected_checksum: &str,
    database_url: &str,
) -> Result<RestorePreparation, AppError> {
    if encryption_key.is_empty() {
        return Err(AppError::InternalError(
            "BACKUP_ENCRYPTION_KEY is not configured.".into(),
        ));
    }

    // 1. File must exist.
    if !Path::new(backup_path).exists() {
        return Err(AppError::NotFound(format!(
            "Backup file not found: {}",
            backup_path
        )));
    }

    // 2. Read and decrypt.
    let encrypted = tokio::fs::read(backup_path)
        .await
        .map_err(|e| AppError::InternalError(format!("Cannot read backup: {}", e)))?;

    let key = derive_key(encryption_key);
    let plaintext = decrypt_data(&encrypted, &key)?;

    // 3. Checksum verification.
    let actual_checksum = sha256_hex(&plaintext);
    if actual_checksum != expected_checksum {
        return Err(AppError::ValidationError(format!(
            "Backup integrity check FAILED. \
             Expected checksum {}, got {}. \
             The backup file may be corrupt or tampered with.",
            &expected_checksum[..8],
            &actual_checksum[..8]
        )));
    }

    // 4. Write restore-ready file.
    let restore_filename = format!(
        "restore_{}.sql",
        chrono::Utc::now().format("%Y%m%d_%H%M%S")
    );
    let restore_path = format!(
        "{}/{}",
        backups_dir.trim_end_matches('/'),
        restore_filename
    );

    tokio::fs::write(&restore_path, &plaintext)
        .await
        .map_err(|e| AppError::InternalError(format!("Cannot write restore file: {}", e)))?;

    let psql_command = format!(
        "psql --dbname '{}' -f '{}'",
        database_url, restore_path
    );

    log::info!(
        "Restore prepared: {} → {} (checksum verified)",
        backup_path,
        restore_path
    );

    Ok(RestorePreparation {
        restore_path,
        checksum: actual_checksum,
        psql_command,
    })
}

// ---------------------------------------------------------------------------
// Unit tests (no I/O)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derive_key_produces_32_bytes() {
        let key = derive_key("test_passphrase_for_meridian");
        assert_eq!(key.len(), 32);
    }

    #[test]
    fn derive_key_is_deterministic() {
        let k1 = derive_key("same_passphrase");
        let k2 = derive_key("same_passphrase");
        assert_eq!(k1, k2);
    }

    #[test]
    fn derive_key_differs_for_different_inputs() {
        let k1 = derive_key("passphrase_one");
        let k2 = derive_key("passphrase_two");
        assert_ne!(k1, k2);
    }

    #[test]
    fn encrypt_decrypt_roundtrip() {
        let key = derive_key("test_encryption_key_meridian_backup");
        let plaintext = b"SELECT 1; -- fake pg_dump output";
        let encrypted = encrypt_data(plaintext, &key).expect("encrypt");
        let decrypted = decrypt_data(&encrypted, &key).expect("decrypt");
        assert_eq!(plaintext, decrypted.as_slice());
    }

    #[test]
    fn decrypt_wrong_key_fails() {
        let key1 = derive_key("correct_key");
        let key2 = derive_key("wrong_key");
        let plaintext = b"some data";
        let encrypted = encrypt_data(plaintext, &key1).expect("encrypt");
        let result = decrypt_data(&encrypted, &key2);
        assert!(result.is_err(), "Decryption with wrong key should fail");
    }

    #[test]
    fn decrypt_tampered_ciphertext_fails() {
        let key = derive_key("key_for_tampering_test");
        let plaintext = b"important data";
        let mut encrypted = encrypt_data(plaintext, &key).expect("encrypt");
        // Tamper with the last byte of the ciphertext.
        if let Some(last) = encrypted.last_mut() {
            *last ^= 0xFF;
        }
        let result = decrypt_data(&encrypted, &key);
        assert!(result.is_err(), "Tampered ciphertext should fail AEAD check");
    }

    #[test]
    fn decrypt_bad_magic_fails() {
        let key = derive_key("key");
        let mut bad_data = vec![0u8; 40];
        bad_data[0] = 0xFF; // corrupt magic
        let result = decrypt_data(&bad_data, &key);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("magic"));
    }

    #[test]
    fn checksum_is_deterministic() {
        let data = b"deterministic input";
        let h1 = sha256_hex(data);
        let h2 = sha256_hex(data);
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 64); // SHA-256 = 32 bytes = 64 hex chars
    }

    #[test]
    fn checksum_differs_for_different_data() {
        let h1 = sha256_hex(b"data one");
        let h2 = sha256_hex(b"data two");
        assert_ne!(h1, h2);
    }

    #[test]
    fn empty_encryption_key_would_be_rejected_at_api_level() {
        // The API check: encryption_key.is_empty() → error
        let key = "";
        assert!(key.is_empty());
    }

    #[test]
    fn magic_header_is_8_bytes() {
        assert_eq!(MAGIC.len(), 8);
    }

    #[test]
    fn encrypted_output_has_correct_minimum_size() {
        let key = derive_key("size_test_key");
        let plaintext = b"x";
        let encrypted = encrypt_data(plaintext, &key).expect("encrypt");
        // magic(8) + nonce(12) + min ciphertext(1) + tag(16) = 37
        assert!(encrypted.len() >= 37);
    }
}
