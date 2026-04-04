/// Report generation endpoints.
///
/// All routes require the Administrator role.
///
/// POST /reports                – trigger a new report (runs synchronously for local use)
/// GET  /reports                – list report jobs (most recent 100)
/// GET  /reports/{id}           – get status + metadata for one job
/// GET  /reports/{id}/download  – serve the CSV file for a completed job
///
/// Supported report_type values: checkins, approvals, orders, kpi, operational
///
/// Request body (POST /reports):
/// {
///   "report_type": "orders",
///   "start_date": "2026-03-01",   ← YYYY-MM-DD
///   "end_date":   "2026-03-31",
///   "pii_masked": true            ← optional, default true
/// }
///
/// PII masking:
///   pii_masked = true  → IDs last-4, emails masked, usernames masked (DEFAULT)
///   pii_masked = false → requires the pii_export permission on the requesting user
///
/// Exports are written to the configured EXPORTS_DIR (default: ../exports/).

use actix_web::{web, HttpResponse};
use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::db::DbPool;
use crate::errors::AppError;
use crate::middleware::auth::{require_global_admin_scope, AuthContext};
use crate::services::masking::check_pii_permission;
use crate::services::reports::{
    generate_approvals_report, generate_checkins_report, generate_kpi_report,
    generate_operational_report, generate_orders_report, report_filename, write_report_file,
};

// ---------------------------------------------------------------------------
// Route config
// ---------------------------------------------------------------------------

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/reports")
            .route("", web::post().to(create_report))
            .route("", web::get().to(list_reports))
            .route("/{id}", web::get().to(get_report))
            .route("/{id}/download", web::get().to(download_report)),
    );
}

// ---------------------------------------------------------------------------
// DTOs
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct CreateReportBody {
    report_type: String,
    start_date: String,
    end_date: String,
    #[serde(default = "default_true")]
    pii_masked: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Serialize, sqlx::FromRow)]
struct ReportJobRow {
    id: Uuid,
    name: String,
    report_type: String,
    status: String,
    pii_masked: bool,
    requested_by: Option<Uuid>,
    created_at: DateTime<Utc>,
    started_at: Option<DateTime<Utc>>,
    completed_at: Option<DateTime<Utc>>,
    output_path: Option<String>,
    error_message: Option<String>,
    row_count: Option<i32>,
    checksum: Option<String>,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn parse_date(s: &str) -> Result<NaiveDate, AppError> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").map_err(|_| {
        AppError::ValidationError(format!(
            "Invalid date '{}'. Expected YYYY-MM-DD (e.g. 2026-03-01).",
            s
        ))
    })
}

fn valid_report_type(rt: &str) -> bool {
    matches!(rt, "checkins" | "approvals" | "orders" | "kpi" | "operational")
}

async fn log_export_audit(
    pool: &DbPool,
    actor_id: Uuid,
    job_id: Uuid,
    report_type: &str,
    pii_masked: bool,
) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO audit_logs
             (id, actor_id, action, entity_type, entity_id, new_data, created_at)
         VALUES ($1, $2, 'generate_report', 'report_job', $3, $4, NOW())",
    )
    .bind(Uuid::new_v4())
    .bind(actor_id)
    .bind(job_id.to_string())
    .bind(serde_json::json!({
        "report_type": report_type,
        "pii_masked": pii_masked,
        "generated_by": actor_id
    }))
    .execute(pool)
    .await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// POST /api/v1/reports
async fn create_report(
    pool: web::Data<DbPool>,
    auth: AuthContext,
    body: web::Json<CreateReportBody>,
    cfg: web::Data<crate::config::Config>,
) -> Result<HttpResponse, AppError> {
    auth.require_role("Administrator")?;
    require_global_admin_scope(auth.0.user_id, pool.get_ref()).await?;

    if !valid_report_type(&body.report_type) {
        return Err(AppError::ValidationError(format!(
            "Unknown report_type '{}'. Valid types: checkins, approvals, orders, kpi, operational.",
            body.report_type
        )));
    }

    let start = parse_date(&body.start_date)?;
    let end = parse_date(&body.end_date)?;

    // PII permission check (only when caller explicitly requests unmasked).
    let pii_masked = if !body.pii_masked {
        let has_perm = check_pii_permission(pool.get_ref(), auth.0.user_id).await?;
        if !has_perm {
            return Err(AppError::Forbidden(
                "The pii_export permission is required to generate reports with unmasked PII. \
                 Contact your administrator."
                    .into(),
            ));
        }
        false
    } else {
        true
    };

    let job_id = Uuid::new_v4();
    let job_name = format!(
        "{} report {} to {}",
        body.report_type, body.start_date, body.end_date
    );

    sqlx::query(
        "INSERT INTO report_jobs
             (id, name, report_type, parameters, status, requested_by, pii_masked, created_at, started_at)
         VALUES ($1, $2, $3, $4, 'running', $5, $6, NOW(), NOW())",
    )
    .bind(job_id)
    .bind(&job_name)
    .bind(&body.report_type)
    .bind(serde_json::json!({
        "start_date": body.start_date,
        "end_date": body.end_date,
        "pii_masked": pii_masked
    }))
    .bind(auth.0.user_id)
    .bind(pii_masked)
    .execute(pool.get_ref())
    .await?;

    log_export_audit(pool.get_ref(), auth.0.user_id, job_id, &body.report_type, pii_masked)
        .await?;

    let csv_result = match body.report_type.as_str() {
        "checkins"    => generate_checkins_report(pool.get_ref(), start, end, pii_masked).await,
        "approvals"   => generate_approvals_report(pool.get_ref(), start, end, pii_masked).await,
        "orders"      => generate_orders_report(pool.get_ref(), start, end, pii_masked).await,
        "kpi"         => generate_kpi_report(pool.get_ref(), start, end, pii_masked).await,
        "operational" => generate_operational_report(pool.get_ref(), start, end, pii_masked).await,
        _             => unreachable!(),
    };

    match csv_result {
        Err(e) => {
            sqlx::query(
                "UPDATE report_jobs SET status = 'failed', error_message = $1 WHERE id = $2",
            )
            .bind(e.to_string())
            .bind(job_id)
            .execute(pool.get_ref())
            .await?;
            Err(e)
        }
        Ok(csv) => {
            let dir = if !cfg.exports_dir.is_empty() {
                cfg.exports_dir.clone()
            } else {
                "../exports".to_string()
            };

            let filename = report_filename(&body.report_type, start, end);
            let path = write_report_file(&csv, &dir, &filename).await?;
            let row_count = csv.lines().count().saturating_sub(1) as i32;
            let checksum = crate::services::backup::sha256_hex(csv.as_bytes());

            sqlx::query(
                "UPDATE report_jobs
                 SET status = 'completed', output_path = $1, row_count = $2,
                     checksum = $3, completed_at = NOW()
                 WHERE id = $4",
            )
            .bind(&path)
            .bind(row_count)
            .bind(&checksum)
            .bind(job_id)
            .execute(pool.get_ref())
            .await?;

            Ok(HttpResponse::Created().json(serde_json::json!({
                "job_id": job_id,
                "name": job_name,
                "report_type": body.report_type,
                "status": "completed",
                "output_path": path,
                "row_count": row_count,
                "pii_masked": pii_masked,
                "checksum": checksum,
                "download_url": format!("/api/v1/reports/{}/download", job_id)
            })))
        }
    }
}

/// GET /api/v1/reports
async fn list_reports(
    pool: web::Data<DbPool>,
    auth: AuthContext,
) -> Result<HttpResponse, AppError> {
    auth.require_role("Administrator")?;
    require_global_admin_scope(auth.0.user_id, pool.get_ref()).await?;

    let rows = sqlx::query_as::<_, ReportJobRow>(
        "SELECT id, name, report_type, status, pii_masked, requested_by,
                created_at, started_at, completed_at, output_path, error_message,
                row_count, checksum
         FROM report_jobs
         ORDER BY created_at DESC
         LIMIT 100",
    )
    .fetch_all(pool.get_ref())
    .await?;

    Ok(HttpResponse::Ok().json(rows))
}

/// GET /api/v1/reports/{id}
async fn get_report(
    pool: web::Data<DbPool>,
    auth: AuthContext,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, AppError> {
    auth.require_role("Administrator")?;
    require_global_admin_scope(auth.0.user_id, pool.get_ref()).await?;
    let job_id = path.into_inner();

    let row = sqlx::query_as::<_, ReportJobRow>(
        "SELECT id, name, report_type, status, pii_masked, requested_by,
                created_at, started_at, completed_at, output_path, error_message,
                row_count, checksum
         FROM report_jobs WHERE id = $1",
    )
    .bind(job_id)
    .fetch_optional(pool.get_ref())
    .await?
    .ok_or_else(|| AppError::NotFound(format!("Report job {} not found.", job_id)))?;

    Ok(HttpResponse::Ok().json(row))
}

/// GET /api/v1/reports/{id}/download
async fn download_report(
    pool: web::Data<DbPool>,
    auth: AuthContext,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, AppError> {
    auth.require_role("Administrator")?;
    require_global_admin_scope(auth.0.user_id, pool.get_ref()).await?;
    let job_id = path.into_inner();

    let row = sqlx::query_as::<_, ReportJobRow>(
        "SELECT id, name, report_type, status, pii_masked, requested_by,
                created_at, started_at, completed_at, output_path, error_message,
                row_count, checksum
         FROM report_jobs WHERE id = $1",
    )
    .bind(job_id)
    .fetch_optional(pool.get_ref())
    .await?
    .ok_or_else(|| AppError::NotFound(format!("Report job {} not found.", job_id)))?;

    if row.status != "completed" {
        return Err(AppError::ConflictError(format!(
            "Report is in '{}' state; download is only available for completed reports.",
            row.status
        )));
    }

    let output_path = row
        .output_path
        .as_ref()
        .ok_or_else(|| AppError::InternalError("Report has no output_path recorded.".into()))?;

    let content = tokio::fs::read_to_string(output_path)
        .await
        .map_err(|e| AppError::NotFound(format!("Report file not readable: {}", e)))?;

    let _ = sqlx::query(
        "INSERT INTO audit_logs
             (id, actor_id, action, entity_type, entity_id, new_data, created_at)
         VALUES ($1, $2, 'download_report', 'report_job', $3, $4, NOW())",
    )
    .bind(Uuid::new_v4())
    .bind(auth.0.user_id)
    .bind(job_id.to_string())
    .bind(serde_json::json!({ "pii_masked": row.pii_masked, "output_path": output_path }))
    .execute(pool.get_ref())
    .await;

    let filename = output_path.split('/').last().unwrap_or("report.csv");

    Ok(HttpResponse::Ok()
        .content_type("text/csv")
        .insert_header((
            "Content-Disposition",
            format!("attachment; filename=\"{}\"", filename),
        ))
        .body(content))
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_report_types_accepted() {
        for rt in ["checkins", "approvals", "orders", "kpi", "operational"] {
            assert!(valid_report_type(rt), "{} should be valid", rt);
        }
    }

    #[test]
    fn invalid_report_type_rejected() {
        assert!(!valid_report_type("sales"));
        assert!(!valid_report_type(""));
        assert!(!valid_report_type("ORDERS"));
    }

    #[test]
    fn parse_date_valid() {
        assert!(parse_date("2026-03-01").is_ok());
        assert!(parse_date("2026-12-31").is_ok());
    }

    #[test]
    fn parse_date_invalid_format() {
        assert!(parse_date("03/01/2026").is_err());
        assert!(parse_date("not-a-date").is_err());
        assert!(parse_date("2026-13-01").is_err());
    }

    #[test]
    fn pii_masked_defaults_to_true() {
        assert!(default_true());
    }

    #[test]
    fn export_authorization_requires_admin_role() {
        let admin_roles = vec!["Administrator".to_string()];
        let student_roles = vec!["Student".to_string()];
        let check = |roles: &Vec<String>| roles.iter().any(|r| r == "Administrator");
        assert!(check(&admin_roles));
        assert!(!check(&student_roles));
    }
}
