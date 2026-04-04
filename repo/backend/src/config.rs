use dotenv::dotenv;
use std::env;

#[derive(Debug, Clone)]
pub struct Config {
    pub database_url: String,
    pub host: String,
    pub port: u16,
    pub session_secret: String,
    pub session_max_age_seconds: i64,
    pub log_level: String,
    /// AES-256 key material for backup encryption.
    /// Must be at least 32 chars; hashed to 32 bytes via SHA-256.
    /// If empty, backup/restore endpoints return 503.
    pub backup_encryption_key: String,
    /// Directory where CSV exports are written.  Defaults to ../exports.
    pub exports_dir: String,
    /// Directory where encrypted backup files are stored.  Defaults to ../backups.
    pub backups_dir: String,
}

impl Config {
    pub fn from_env() -> Self {
        dotenv().ok();

        let database_url = env::var("DATABASE_URL").unwrap_or_else(|_| {
            panic!("DATABASE_URL environment variable is required but not set. Please set it to your PostgreSQL connection string, e.g. postgres://user:password@localhost/meridian");
        });

        let host = env::var("HOST").unwrap_or_else(|_| "127.0.0.1".to_string());

        let port = env::var("PORT")
            .unwrap_or_else(|_| "8080".to_string())
            .parse::<u16>()
            .unwrap_or_else(|_| panic!("PORT must be a valid u16 integer"));

        let session_secret = env::var("SESSION_SECRET").unwrap_or_else(|_| {
            panic!("SESSION_SECRET environment variable is required but not set. It must be at least 64 characters long.");
        });

        if session_secret.len() < 64 {
            panic!(
                "SESSION_SECRET is too short ({} chars). It must be at least 64 characters long for security.",
                session_secret.len()
            );
        }

        let session_max_age_seconds = env::var("SESSION_MAX_AGE_SECONDS")
            .unwrap_or_else(|_| "3600".to_string())
            .parse::<i64>()
            .unwrap_or_else(|_| panic!("SESSION_MAX_AGE_SECONDS must be a valid i64 integer"));

        let log_level = env::var("LOG_LEVEL").unwrap_or_else(|_| "info".to_string());

        let backup_encryption_key =
            env::var("BACKUP_ENCRYPTION_KEY").unwrap_or_default();

        let exports_dir = env::var("EXPORTS_DIR").unwrap_or_else(|_| "../exports".to_string());
        let backups_dir = env::var("BACKUPS_DIR").unwrap_or_else(|_| "../backups".to_string());

        Config {
            database_url,
            host,
            port,
            session_secret,
            session_max_age_seconds,
            log_level,
            backup_encryption_key,
            exports_dir,
            backups_dir,
        }
    }
}
