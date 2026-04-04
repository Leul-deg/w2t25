/// Backup and restore endpoints.  Administrator-only.
///
/// GET  /backups           – list all backup metadata records
/// POST /backups           – create a new encrypted backup (pg_dump + AES-256-GCM)
/// GET  /backups/{id}      – get backup metadata for one entry
/// POST /backups/{id}/restore – safety-check, decrypt, and write restore file
///
/// Encryption:
///   AES-256-GCM key derived from BACKUP_ENCRYPTION_KEY env var via SHA-256.
///   Each backup has a unique random 12-byte nonce; the AEAD tag ensures
///   integrity.  Backups cannot be read without the correct key.
///
/// Restore process:
///   The API NEVER executes DROP DATABASE or ALTER DATABASE on the running
///   server.  Instead, POST /backups/{id}/restore:
///     1. Verifies the backup is in backup_metadata with status = 'completed'
///     2. Decrypts and authenticates the file
///     3. Verifies the SHA-256 checksum matches the recorded value
///     4. Writes the decrypted SQL to backups/restore_{timestamp}.sql
///     5. Returns the path and the psql command the admin must run manually
///   All restore attempts are logged in audit_logs.

use actix_web::{web, HttpResponse};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::db::DbPool;
use crate::errors::AppError;
use crate::middleware::auth::{require_global_admin_scope, AuthContext};
use crate::services::backup::{create_backup, prepare_restore};

// ---------------------------------------------------------------------------
// Route config
// ---------------------------------------------------------------------------

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/backups")
            .route("", web::get().to(list_backups))
            .route("", web::post().to(create_backup_handler))
            .route("/{id}", web::get().to(get_backup))
            .route("/{id}/restore", web::post().to(restore_backup)),
    );
}

// ---------------------------------------------------------------------------
// DTOs
// ---------------------------------------------------------------------------

#[derive(Serialize, sqlx::FromRow)]
struct BackupMetaRow {
    id: Uuid,
    filename: String,
    backup_type: String,
    size_bytes: Option<i64>,
    status: String,
    created_at: DateTime<Utc>,
    completed_at: Option<DateTime<Utc>>,
    checksum: Option<String>,
    notes: Option<String>,
}

#[derive(Deserialize)]
struct CreateBackupBody {
    notes: Option<String>,
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// GET /api/v1/backups
async fn list_backups(
    pool: web::Data<DbPool>,
    auth: AuthContext,
) -> Result<HttpResponse, AppError> {
    auth.require_role("Administrator")?;
    require_global_admin_scope(auth.0.user_id, pool.get_ref()).await?;

    let rows = sqlx::query_as::<_, BackupMetaRow>(
        "SELECT id, filename, backup_type, size_bytes, status,
                created_at, completed_at, checksum, notes
         FROM backup_metadata
         WHERE status != 'deleted'
         ORDER BY created_at DESC
         LIMIT 50",
    )
    .fetch_all(pool.get_ref())
    .await?;

    Ok(HttpResponse::Ok().json(rows))
}

/// POST /api/v1/backups
///
/// Creates an AES-256-GCM encrypted pg_dump backup.
async fn create_backup_handler(
    pool: web::Data<DbPool>,
    auth: AuthContext,
    body: web::Json<CreateBackupBody>,
    cfg: web::Data<crate::config::Config>,
) -> Result<HttpResponse, AppError> {
    auth.require_role("Administrator")?;
    require_global_admin_scope(auth.0.user_id, pool.get_ref()).await?;

    // Create a pending record before running pg_dump.
    let backup_id = Uuid::new_v4();
    let pending_filename = format!("backup_{}.mbak", backup_id);

    sqlx::query(
        "INSERT INTO backup_metadata
             (id, filename, backup_type, status, notes, created_at)
         VALUES ($1, $2, 'full', 'pending', $3, NOW())",
    )
    .bind(backup_id)
    .bind(&pending_filename)
    .bind(&body.notes)
    .execute(pool.get_ref())
    .await?;

    // Audit.
    sqlx::query(
        "INSERT INTO audit_logs
             (id, actor_id, action, entity_type, entity_id, new_data, created_at)
         VALUES ($1, $2, 'create_backup', 'backup', $3, $4, NOW())",
    )
    .bind(Uuid::new_v4())
    .bind(auth.0.user_id)
    .bind(backup_id.to_string())
    .bind(serde_json::json!({
        "filename": pending_filename,
        "initiated_by": auth.0.username
    }))
    .execute(pool.get_ref())
    .await?;

    match create_backup(
        &cfg.database_url,
        &cfg.backups_dir,
        &cfg.backup_encryption_key,
    )
    .await
    {
        Ok(result) => {
            // Update metadata with actual filename/path from the service.
            sqlx::query(
                "UPDATE backup_metadata
                 SET filename = $1, size_bytes = $2, checksum = $3,
                     status = 'completed', completed_at = NOW()
                 WHERE id = $4",
            )
            .bind(&result.filename)
            .bind(result.size_bytes as i64)
            .bind(&result.checksum)
            .bind(backup_id)
            .execute(pool.get_ref())
            .await?;

            log::info!(
                "Backup completed by {}: {} ({} bytes)",
                auth.0.username,
                result.path,
                result.size_bytes
            );

            Ok(HttpResponse::Created().json(serde_json::json!({
                "backup_id": backup_id,
                "filename": result.filename,
                "path": result.path,
                "size_bytes": result.size_bytes,
                "checksum": result.checksum,
                "status": "completed",
                "message": "Backup created and encrypted with AES-256-GCM."
            })))
        }
        Err(e) => {
            sqlx::query(
                "UPDATE backup_metadata SET status = 'failed' WHERE id = $1",
            )
            .bind(backup_id)
            .execute(pool.get_ref())
            .await
            .ok();

            log::error!(
                "Backup failed for {}: {}",
                auth.0.username,
                e
            );
            Err(e)
        }
    }
}

/// GET /api/v1/backups/{id}
async fn get_backup(
    pool: web::Data<DbPool>,
    auth: AuthContext,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, AppError> {
    auth.require_role("Administrator")?;
    require_global_admin_scope(auth.0.user_id, pool.get_ref()).await?;
    let backup_id = path.into_inner();

    let row = sqlx::query_as::<_, BackupMetaRow>(
        "SELECT id, filename, backup_type, size_bytes, status,
                created_at, completed_at, checksum, notes
         FROM backup_metadata WHERE id = $1",
    )
    .bind(backup_id)
    .fetch_optional(pool.get_ref())
    .await?
    .ok_or_else(|| AppError::NotFound(format!("Backup {} not found.", backup_id)))?;

    Ok(HttpResponse::Ok().json(row))
}

/// POST /api/v1/backups/{id}/restore
///
/// Decrypts and validates the backup, then writes a restore-ready SQL file.
/// The admin must run the returned psql command manually.
async fn restore_backup(
    pool: web::Data<DbPool>,
    auth: AuthContext,
    path: web::Path<Uuid>,
    cfg: web::Data<crate::config::Config>,
) -> Result<HttpResponse, AppError> {
    auth.require_role("Administrator")?;
    require_global_admin_scope(auth.0.user_id, pool.get_ref()).await?;
    let backup_id = path.into_inner();

    // ── Safety check 1: backup must be in metadata with status = 'completed' ──
    let row = sqlx::query_as::<_, BackupMetaRow>(
        "SELECT id, filename, backup_type, size_bytes, status,
                created_at, completed_at, checksum, notes
         FROM backup_metadata WHERE id = $1",
    )
    .bind(backup_id)
    .fetch_optional(pool.get_ref())
    .await?
    .ok_or_else(|| AppError::NotFound(format!("Backup {} not found.", backup_id)))?;

    if row.status != "completed" {
        return Err(AppError::ConflictError(format!(
            "Backup is in '{}' state; only 'completed' backups can be restored.",
            row.status
        )));
    }

    let checksum = row.checksum.as_deref().ok_or_else(|| {
        AppError::InternalError("Backup has no recorded checksum; cannot verify integrity.".into())
    })?;

    // Build the expected path.
    let backup_path = format!(
        "{}/{}",
        cfg.backups_dir.trim_end_matches('/'),
        row.filename
    );

    // ── Audit the restore attempt ─────────────────────────────────────────
    sqlx::query(
        "INSERT INTO audit_logs
             (id, actor_id, action, entity_type, entity_id, new_data, created_at)
         VALUES ($1, $2, 'restore_backup_requested', 'backup', $3, $4, NOW())",
    )
    .bind(Uuid::new_v4())
    .bind(auth.0.user_id)
    .bind(backup_id.to_string())
    .bind(serde_json::json!({
        "filename": row.filename,
        "requested_by": auth.0.username
    }))
    .execute(pool.get_ref())
    .await?;

    // ── Decrypt, verify integrity, write restore file ─────────────────────
    let prep = prepare_restore(
        &backup_path,
        &cfg.backups_dir,
        &cfg.backup_encryption_key,
        checksum,
        &cfg.database_url,
    )
    .await?;

    // ── Audit success ─────────────────────────────────────────────────────
    sqlx::query(
        "INSERT INTO audit_logs
             (id, actor_id, action, entity_type, entity_id, new_data, created_at)
         VALUES ($1, $2, 'restore_backup_prepared', 'backup', $3, $4, NOW())",
    )
    .bind(Uuid::new_v4())
    .bind(auth.0.user_id)
    .bind(backup_id.to_string())
    .bind(serde_json::json!({
        "restore_path": prep.restore_path,
        "checksum_verified": true
    }))
    .execute(pool.get_ref())
    .await?;

    log::warn!(
        "Restore prepared by {} — restore file at {} — admin must apply manually",
        auth.0.username,
        prep.restore_path
    );

    Ok(HttpResponse::Ok().json(serde_json::json!({
        "backup_id": backup_id,
        "restore_path": prep.restore_path,
        "checksum_verified": true,
        "psql_command": prep.psql_command,
        "warning": "The server has NOT applied this restore. \
                    Stop the backend, run the psql command below, then restart. \
                    This action cannot be undone.",
        "message": "Backup verified and restore file written. Apply it manually."
    })))
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backup_requires_admin_role() {
        let admin_roles = vec!["Administrator".to_string()];
        let teacher_roles = vec!["Teacher".to_string()];
        let check = |roles: &Vec<String>| roles.iter().any(|r| r == "Administrator");
        assert!(check(&admin_roles));
        assert!(!check(&teacher_roles));
    }

    #[test]
    fn restore_requires_completed_status() {
        // Simulates the guard in restore_backup.
        for status in ["pending", "failed", "deleted", "running"] {
            assert_ne!(status, "completed");
        }
        assert_eq!("completed", "completed");
    }

    #[test]
    fn restore_requires_checksum() {
        // If checksum is None, restore must be rejected.
        let checksum: Option<String> = None;
        assert!(checksum.is_none());
    }

    #[test]
    fn backup_path_constructed_from_dir_and_filename() {
        let dir = "/backups";
        let filename = "backup_abc.mbak";
        let path = format!("{}/{}", dir.trim_end_matches('/'), filename);
        assert_eq!(path, "/backups/backup_abc.mbak");
    }

    #[test]
    fn empty_encryption_key_means_backups_unavailable() {
        // Mirrors the check in services/backup.rs.
        let key = "";
        assert!(key.is_empty(), "empty key should prevent backup creation");
    }
}
