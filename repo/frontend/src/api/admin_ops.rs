use serde::{Deserialize, Serialize};

use super::client::{get, post, ApiError};

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct ReportJob {
    pub id: String,
    pub name: String,
    pub report_type: String,
    pub status: String,
    pub pii_masked: bool,
    pub created_at: String,
    pub completed_at: Option<String>,
    pub output_path: Option<String>,
    pub error_message: Option<String>,
    pub row_count: Option<i32>,
    pub checksum: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CreateReportRequest {
    pub report_type: String,
    pub start_date: String,
    pub end_date: String,
    pub pii_masked: bool,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct CreateReportResponse {
    pub job_id: String,
    pub name: String,
    pub report_type: String,
    pub status: String,
    pub output_path: String,
    pub row_count: i32,
    pub pii_masked: bool,
    pub checksum: String,
    pub download_url: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct BackupMeta {
    pub id: String,
    pub filename: String,
    pub backup_type: String,
    pub size_bytes: Option<i64>,
    pub status: String,
    pub created_at: String,
    pub completed_at: Option<String>,
    pub checksum: Option<String>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CreateBackupRequest {
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct CreateBackupResponse {
    pub backup_id: String,
    pub filename: String,
    pub path: String,
    pub size_bytes: u64,
    pub checksum: String,
    pub status: String,
    pub message: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct RestoreBackupResponse {
    pub backup_id: String,
    pub restore_path: String,
    pub checksum_verified: bool,
    pub psql_command: String,
    pub warning: String,
    pub message: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct LogListResponse<T> {
    pub count: usize,
    pub limit: i64,
    pub offset: i64,
    pub rows: Vec<T>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct AuditLogRow {
    pub id: String,
    pub actor_username: Option<String>,
    pub action: String,
    pub entity_type: String,
    pub entity_id: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct AccessLogRow {
    pub id: String,
    pub username: Option<String>,
    pub action: String,
    pub ip_address: Option<String>,
    pub success: bool,
    pub details: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct ErrorLogRow {
    pub id: String,
    pub level: String,
    pub message: String,
    pub request_path: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct AdminUser {
    pub id: String,
    pub username: String,
    pub email: String,
    pub display_name: Option<String>,
    pub account_state: String,
    pub created_at: String,
    pub roles: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SetUserStateRequest {
    pub state: String,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct DeletionRequestRow {
    pub id: String,
    pub user_id: String,
    pub username: String,
    pub email: String,
    pub reason: Option<String>,
    pub status: String,
    pub requested_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RejectDeletionRequest {
    pub reason: Option<String>,
}

pub async fn list_reports(token: &str) -> Result<Vec<ReportJob>, ApiError> {
    get("/reports", Some(token)).await
}

pub async fn create_report(
    token: &str,
    req: &CreateReportRequest,
) -> Result<CreateReportResponse, ApiError> {
    post("/reports", req, Some(token)).await
}

pub async fn list_backups(token: &str) -> Result<Vec<BackupMeta>, ApiError> {
    get("/backups", Some(token)).await
}

pub async fn create_backup(
    token: &str,
    req: &CreateBackupRequest,
) -> Result<CreateBackupResponse, ApiError> {
    post("/backups", req, Some(token)).await
}

pub async fn restore_backup(
    token: &str,
    backup_id: &str,
) -> Result<RestoreBackupResponse, ApiError> {
    post(
        &format!("/backups/{}/restore", backup_id),
        &serde_json::json!({}),
        Some(token),
    )
    .await
}

pub async fn audit_logs(token: &str) -> Result<LogListResponse<AuditLogRow>, ApiError> {
    get("/logs/audit", Some(token)).await
}

pub async fn access_logs(token: &str) -> Result<LogListResponse<AccessLogRow>, ApiError> {
    get("/logs/access", Some(token)).await
}

pub async fn error_logs(token: &str) -> Result<LogListResponse<ErrorLogRow>, ApiError> {
    get("/logs/errors", Some(token)).await
}

pub async fn prune_logs(token: &str) -> Result<serde_json::Value, ApiError> {
    post("/logs/prune", &serde_json::json!({}), Some(token)).await
}

pub async fn list_admin_users(token: &str) -> Result<Vec<AdminUser>, ApiError> {
    get("/admin/users", Some(token)).await
}

pub async fn set_admin_user_state(
    token: &str,
    user_id: &str,
    req: &SetUserStateRequest,
) -> Result<serde_json::Value, ApiError> {
    post(&format!("/admin/users/{}/set-state", user_id), req, Some(token)).await
}

pub async fn list_deletion_requests(token: &str) -> Result<Vec<DeletionRequestRow>, ApiError> {
    get("/admin/deletion-requests", Some(token)).await
}

pub async fn approve_deletion_request(
    token: &str,
    request_id: &str,
) -> Result<serde_json::Value, ApiError> {
    post(
        &format!("/admin/deletion-requests/{}/approve", request_id),
        &serde_json::json!({}),
        Some(token),
    )
    .await
}

pub async fn reject_deletion_request(
    token: &str,
    request_id: &str,
    req: &RejectDeletionRequest,
) -> Result<serde_json::Value, ApiError> {
    post(
        &format!("/admin/deletion-requests/{}/reject", request_id),
        req,
        Some(token),
    )
    .await
}
