use actix_web::{web, HttpRequest, HttpResponse};
use chrono::{DateTime, NaiveDate, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::db::DbPool;
use crate::errors::AppError;
use crate::middleware::auth::AuthContext;
use crate::services::notifications::create_user_notification;

// ---------------------------------------------------------------------------
// Route configuration
// ---------------------------------------------------------------------------

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/check-ins")
            .route("/windows", web::get().to(list_windows))
            .route("/windows/{window_id}", web::get().to(get_window))
            .route("/windows/{window_id}/submit", web::post().to(submit_checkin))
            .route(
                "/windows/{window_id}/submissions",
                web::get().to(list_submissions),
            )
            .route(
                "/windows/{window_id}/homerooms",
                web::get().to(list_window_homerooms),
            )
            .route(
                "/windows/{window_id}/submissions/{submission_id}/decide",
                web::post().to(decide_submission),
            )
            .route("/my", web::get().to(my_checkins)),
    );
}

// ---------------------------------------------------------------------------
// Shared types
// ---------------------------------------------------------------------------

#[derive(sqlx::FromRow, Serialize)]
struct CheckinWindowRow {
    id: Uuid,
    title: String,
    description: Option<String>,
    opens_at: DateTime<Utc>,
    closes_at: DateTime<Utc>,
    allow_late: bool,
    active: bool,
    school_id: Uuid,
    school_name: String,
}

/// Window row enriched with a computed status string.
#[derive(Serialize)]
struct CheckinWindowWithStatus {
    id: Uuid,
    title: String,
    description: Option<String>,
    opens_at: DateTime<Utc>,
    closes_at: DateTime<Utc>,
    allow_late: bool,
    active: bool,
    school_id: Uuid,
    school_name: String,
    /// "upcoming" | "open" | "accepting_late" | "closed"
    status: &'static str,
}

impl From<CheckinWindowRow> for CheckinWindowWithStatus {
    fn from(w: CheckinWindowRow) -> Self {
        let now = Utc::now();
        let status = if !w.active {
            "closed"
        } else if now < w.opens_at {
            "upcoming"
        } else if now <= w.closes_at {
            "open"
        } else if w.allow_late {
            "accepting_late"
        } else {
            "closed"
        };
        Self {
            id: w.id,
            title: w.title,
            description: w.description,
            opens_at: w.opens_at,
            closes_at: w.closes_at,
            allow_late: w.allow_late,
            active: w.active,
            school_id: w.school_id,
            school_name: w.school_name,
            status,
        }
    }
}

#[derive(Deserialize)]
struct SubmitCheckinBody {
    notes: Option<String>,
    /// Required when caller has the Parent role. Must name a student linked via
    /// parent_student_links. Ignored for Student callers.
    student_id: Option<Uuid>,
}

#[derive(sqlx::FromRow, Serialize)]
struct SubmissionRow {
    submission_id: Uuid,
    window_id: Uuid,
    window_title: String,
    student_id: Uuid,
    username: String,
    display_name: Option<String>,
    submitted_at: DateTime<Utc>,
    is_late: bool,
    method: String,
    notes: Option<String>,
    decision: String,
    reason: Option<String>,
    decided_at: Option<DateTime<Utc>>,
    decided_by_name: Option<String>,
}

#[derive(Deserialize)]
struct DecideBody {
    decision: String,
    reason: Option<String>,
}

/// Query parameters for `GET /windows/{window_id}/submissions`.
#[derive(Deserialize, Default)]
struct SubmissionQuery {
    /// Restrict results to students enrolled at this school.
    school_id: Option<Uuid>,
    /// Filter to students enrolled in this homeroom/class.
    homeroom_id: Option<Uuid>,
    /// Lower bound for `submitted_at` (inclusive), format `YYYY-MM-DD`.
    date_from: Option<String>,
    /// Upper bound for `submitted_at` (inclusive), format `YYYY-MM-DD`.
    date_to: Option<String>,
    /// Filter by decision state: `pending` | `approved` | `rejected`.
    decision: Option<String>,
}

#[derive(sqlx::FromRow, Serialize)]
struct HomeroomRow {
    id: Uuid,
    name: String,
    grade_level: Option<String>,
}

#[derive(sqlx::FromRow, Serialize)]
struct MyCheckinRow {
    submission_id: Uuid,
    window_id: Uuid,
    window_title: String,
    opens_at: DateTime<Utc>,
    closes_at: DateTime<Utc>,
    submitted_at: DateTime<Utc>,
    is_late: bool,
    method: String,
    decision: String,
    reason: Option<String>,
    decided_at: Option<DateTime<Utc>>,
    student_username: String,
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// GET /api/v1/check-ins/windows
///
/// Admins see all windows. Everyone else is scoped to schools they are assigned
/// to directly (user_school_assignments) or via class enrolment (students).
async fn list_windows(
    pool: web::Data<DbPool>,
    auth: AuthContext,
) -> Result<HttpResponse, AppError> {
    auth.require_any_role(&[
        "Administrator",
        "Teacher",
        "AcademicStaff",
        "Student",
        "Parent",
    ])?;

    let rows: Vec<CheckinWindowRow> = if auth.is_admin() {
        sqlx::query_as::<_, CheckinWindowRow>(
            "SELECT cw.id, cw.title, cw.description, cw.opens_at, cw.closes_at,
                    cw.allow_late, cw.active, cw.school_id, s.name as school_name
             FROM checkin_windows cw
             JOIN schools s ON cw.school_id = s.id
             ORDER BY cw.opens_at DESC
             LIMIT 100",
        )
        .fetch_all(pool.get_ref())
        .await?
    } else {
        // Students: direct assignment OR class enrolment.
        // Parents:  direct assignment OR via any linked student's enrolment.
        // Teachers / AcademicStaff: direct assignment only.
        sqlx::query_as::<_, CheckinWindowRow>(
            "SELECT DISTINCT cw.id, cw.title, cw.description, cw.opens_at, cw.closes_at,
                    cw.allow_late, cw.active, cw.school_id, s.name as school_name
             FROM checkin_windows cw
             JOIN schools s ON cw.school_id = s.id
             WHERE cw.school_id IN (
               SELECT school_id FROM user_school_assignments WHERE user_id = $1
               UNION
               SELECT c.school_id FROM class_enrollments ce
               JOIN classes c ON ce.class_id = c.id
               WHERE ce.student_id = $1 AND ce.status = 'active'
               UNION
               SELECT c.school_id FROM parent_student_links psl
               JOIN class_enrollments ce ON ce.student_id = psl.student_id
               JOIN classes c ON ce.class_id = c.id
               WHERE psl.parent_id = $1 AND ce.status = 'active'
             )
             ORDER BY cw.opens_at DESC
             LIMIT 100",
        )
        .bind(auth.0.user_id)
        .fetch_all(pool.get_ref())
        .await?
    };

    let windows: Vec<CheckinWindowWithStatus> = rows.into_iter().map(Into::into).collect();
    Ok(HttpResponse::Ok().json(windows))
}

/// GET /api/v1/check-ins/windows/{window_id}
async fn get_window(
    pool: web::Data<DbPool>,
    auth: AuthContext,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, AppError> {
    let window_id = path.into_inner();

    let window = sqlx::query_as::<_, CheckinWindowRow>(
        "SELECT cw.id, cw.title, cw.description, cw.opens_at, cw.closes_at,
                cw.allow_late, cw.active, cw.school_id, s.name as school_name
         FROM checkin_windows cw
         JOIN schools s ON cw.school_id = s.id
         WHERE cw.id = $1",
    )
    .bind(window_id)
    .fetch_optional(pool.get_ref())
    .await?
    .ok_or_else(|| AppError::NotFound("Check-in window not found".into()))?;

    if !auth.is_admin() {
        let school_id = sqlx::query_scalar::<_, Uuid>(
            "SELECT school_id FROM checkin_windows WHERE id = $1",
        )
        .bind(window_id)
        .fetch_one(pool.get_ref())
        .await?;

        require_checkin_view_access(&auth, pool.get_ref(), school_id).await?;
    }

    let with_status: CheckinWindowWithStatus = window.into();
    Ok(HttpResponse::Ok().json(with_status))
}

/// POST /api/v1/check-ins/windows/{window_id}/submit
///
/// Student: submits for themselves (student_id = auth.user_id, method = 'manual').
/// Parent:  must supply student_id; verified against parent_student_links (method = 'parent').
async fn submit_checkin(
    pool: web::Data<DbPool>,
    req: HttpRequest,
    auth: AuthContext,
    path: web::Path<Uuid>,
    body: web::Json<SubmitCheckinBody>,
) -> Result<HttpResponse, AppError> {
    auth.require_any_role(&["Student", "Parent"])?;

    let window_id = path.into_inner();
    let now = Utc::now();

    // --- Resolve student_id and method -------------------------------------------
    let is_parent = auth.0.roles.iter().any(|r| r == "Parent");

    let (student_id, method) = if is_parent {
        let sid = body.student_id.ok_or_else(|| {
            AppError::ValidationError(
                "student_id is required when submitting on behalf of a student.".into(),
            )
        })?;

        let linked: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM parent_student_links
             WHERE parent_id = $1 AND student_id = $2",
        )
        .bind(auth.0.user_id)
        .bind(sid)
        .fetch_one(pool.get_ref())
        .await?;

        if linked == 0 {
            return Err(AppError::Forbidden(
                "You are not linked to this student account.".into(),
            ));
        }
        (sid, "parent")
    } else {
        (auth.0.user_id, "manual")
    };

    // --- Load window ---------------------------------------------------------------
    #[derive(sqlx::FromRow)]
    struct WindowMeta {
        school_id: Uuid,
        title: String,
        opens_at: DateTime<Utc>,
        closes_at: DateTime<Utc>,
        allow_late: bool,
        active: bool,
    }

    let window = sqlx::query_as::<_, WindowMeta>(
        "SELECT school_id, title, opens_at, closes_at, allow_late, active
         FROM checkin_windows WHERE id = $1",
    )
    .bind(window_id)
    .fetch_optional(pool.get_ref())
    .await?
    .ok_or_else(|| AppError::NotFound("Check-in window not found".into()))?;

    // --- Business rule checks ------------------------------------------------------

    if !window.active {
        return Err(AppError::ValidationError(
            "Check-in window is not active.".into(),
        ));
    }
    if now < window.opens_at {
        return Err(AppError::ValidationError(
            "Check-in window has not opened yet.".into(),
        ));
    }
    let is_late = now > window.closes_at;
    if is_late && !window.allow_late {
        return Err(AppError::ValidationError(
            "Check-in window is closed.".into(),
        ));
    }

    // Student must be enrolled at the window's school.
    require_student_school_access(pool.get_ref(), student_id, window.school_id).await?;

    // Prevent duplicate submission for same (window, student).
    let existing: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM checkin_submissions
         WHERE window_id = $1 AND student_id = $2",
    )
    .bind(window_id)
    .bind(student_id)
    .fetch_one(pool.get_ref())
    .await?;

    if existing > 0 {
        return Err(AppError::ConflictError(
            "A check-in has already been submitted for this window.".into(),
        ));
    }

    // --- Persist submission ---------------------------------------------------------
    let ip = extract_ip(&req);
    let submission_id = Uuid::new_v4();

    sqlx::query(
        "INSERT INTO checkin_submissions
         (id, window_id, student_id, submitted_at, method, notes, ip_address, is_late)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
    )
    .bind(submission_id)
    .bind(window_id)
    .bind(student_id)
    .bind(now)
    .bind(method)
    .bind(body.notes.as_deref())
    .bind(ip.as_deref())
    .bind(is_late)
    .execute(pool.get_ref())
    .await?;

    // Insert initial pending decision.
    sqlx::query(
        "INSERT INTO checkin_approval_decisions
         (id, submission_id, decided_by, decision, decided_at)
         VALUES ($1, $2, $3, 'pending', $4)",
    )
    .bind(Uuid::new_v4())
    .bind(submission_id)
    .bind(auth.0.user_id)
    .bind(now)
    .execute(pool.get_ref())
    .await?;

    // Audit log.
    log_audit(
        pool.get_ref(),
        auth.0.user_id,
        "checkin_submitted",
        "checkin_submission",
        &submission_id.to_string(),
        None,
        Some(serde_json::json!({
            "window_id": window_id,
            "student_id": student_id,
            "method": method,
            "is_late": is_late,
        })),
    )
    .await?;

    log::info!(
        "checkin submitted: submission_id={} student={} window={} method={} is_late={}",
        submission_id,
        student_id,
        window_id,
        method,
        is_late
    );

    Ok(HttpResponse::Created().json(serde_json::json!({
        "submission_id": submission_id,
        "window_id": window_id,
        "window_title": window.title,
        "student_id": student_id,
        "submitted_at": now,
        "is_late": is_late,
        "status": "pending"
    })))
}

/// GET /api/v1/check-ins/windows/{window_id}/submissions
///
/// Returns submissions for the given window with optional server-side filtering.
///
/// Query params (all optional):
///   `school_id`   – only students enrolled at this school
///   `homeroom_id` – only students enrolled in this class/homeroom
///   `date_from`   – submitted_at >= YYYY-MM-DD (start of day UTC)
///   `date_to`     – submitted_at <= YYYY-MM-DD (end of day UTC)
///   `decision`    – `pending` | `approved` | `rejected`
///
/// Admin: unrestricted. Teacher / AcademicStaff: must be assigned to the school.
async fn list_submissions(
    pool: web::Data<DbPool>,
    auth: AuthContext,
    path: web::Path<Uuid>,
    query: web::Query<SubmissionQuery>,
) -> Result<HttpResponse, AppError> {
    auth.require_any_role(&["Administrator", "Teacher", "AcademicStaff"])?;

    let window_id = path.into_inner();

    let school_id = sqlx::query_scalar::<_, Uuid>(
        "SELECT school_id FROM checkin_windows WHERE id = $1",
    )
    .bind(window_id)
    .fetch_optional(pool.get_ref())
    .await?
    .ok_or_else(|| AppError::NotFound("Check-in window not found".into()))?;

    if !auth.is_admin() {
        crate::middleware::auth::require_school_access(&auth, pool.get_ref(), school_id).await?;
    }

    // Parse optional date bounds.
    let parse_date = |s: &str| -> Result<NaiveDate, AppError> {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").map_err(|_| {
            AppError::ValidationError(format!(
                "Invalid date '{}'. Expected YYYY-MM-DD.",
                s
            ))
        })
    };
    let date_from: Option<DateTime<Utc>> = query
        .date_from
        .as_deref()
        .map(|s| parse_date(s))
        .transpose()?
        .map(|d| Utc.from_utc_datetime(&d.and_hms_opt(0, 0, 0).unwrap()));
    let date_to: Option<DateTime<Utc>> = query
        .date_to
        .as_deref()
        .map(|s| parse_date(s))
        .transpose()?
        .map(|d| Utc.from_utc_datetime(&d.and_hms_opt(23, 59, 59).unwrap()));

    // Validate decision filter if supplied.
    if let Some(ref d) = query.decision {
        if !matches!(d.as_str(), "pending" | "approved" | "rejected") {
            return Err(AppError::ValidationError(
                "decision must be 'pending', 'approved', or 'rejected'.".into(),
            ));
        }
    }

    // Build dynamic query.
    let mut qb = sqlx::QueryBuilder::new(
        "SELECT
           cs.id            AS submission_id,
           cs.window_id,
           cw.title         AS window_title,
           cs.student_id,
           u.username,
           u.display_name,
           cs.submitted_at,
           cs.is_late,
           cs.method,
           cs.notes,
           COALESCE(cad.decision, 'pending') AS decision,
           cad.reason,
           cad.decided_at,
           reviewer.username AS decided_by_name
         FROM checkin_submissions cs
         JOIN users u             ON cs.student_id = u.id
         JOIN checkin_windows cw  ON cs.window_id  = cw.id
         LEFT JOIN checkin_approval_decisions cad ON cad.submission_id = cs.id
         LEFT JOIN users reviewer ON cad.decided_by = reviewer.id
         WHERE cs.window_id = ",
    );
    qb.push_bind(window_id);

    if let Some(sid) = query.school_id {
        qb.push(
            " AND cs.student_id IN \
             (SELECT user_id FROM user_school_assignments WHERE school_id = ",
        );
        qb.push_bind(sid);
        qb.push(")");
    }
    if let Some(homeroom_id) = query.homeroom_id {
        qb.push(
            " AND cs.student_id IN \
             (SELECT student_id FROM class_enrollments WHERE class_id = ",
        );
        qb.push_bind(homeroom_id);
        qb.push(")");
    }
    if let Some(from) = date_from {
        qb.push(" AND cs.submitted_at >= ");
        qb.push_bind(from);
    }
    if let Some(to) = date_to {
        qb.push(" AND cs.submitted_at <= ");
        qb.push_bind(to);
    }
    if let Some(ref decision) = query.decision {
        qb.push(" AND COALESCE(cad.decision, 'pending') = ");
        qb.push_bind(decision.clone());
    }

    qb.push(" ORDER BY cs.submitted_at ASC");

    let submissions = qb
        .build_query_as::<SubmissionRow>()
        .fetch_all(pool.get_ref())
        .await?;

    Ok(HttpResponse::Ok().json(submissions))
}

/// GET /api/v1/check-ins/windows/{window_id}/homerooms
///
/// Returns the active classes (homerooms) for the window's school.
/// Used by the review UI to populate the homeroom filter dropdown.
async fn list_window_homerooms(
    pool: web::Data<DbPool>,
    auth: AuthContext,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, AppError> {
    auth.require_any_role(&["Administrator", "Teacher", "AcademicStaff"])?;

    let window_id = path.into_inner();

    let school_id = sqlx::query_scalar::<_, Uuid>(
        "SELECT school_id FROM checkin_windows WHERE id = $1",
    )
    .bind(window_id)
    .fetch_optional(pool.get_ref())
    .await?
    .ok_or_else(|| AppError::NotFound("Check-in window not found".into()))?;

    if !auth.is_admin() {
        crate::middleware::auth::require_school_access(&auth, pool.get_ref(), school_id).await?;
    }

    let homerooms = sqlx::query_as::<_, HomeroomRow>(
        "SELECT id, name, grade_level
         FROM classes
         WHERE school_id = $1 AND active = TRUE
         ORDER BY name",
    )
    .bind(school_id)
    .fetch_all(pool.get_ref())
    .await?;

    Ok(HttpResponse::Ok().json(homerooms))
}

/// POST /api/v1/check-ins/windows/{window_id}/submissions/{submission_id}/decide
///
/// Approve or reject a pending submission. Rejection requires a non-empty reason.
/// Decisions are final — re-deciding an already-decided submission returns 409.
async fn decide_submission(
    pool: web::Data<DbPool>,
    auth: AuthContext,
    path: web::Path<(Uuid, Uuid)>,
    body: web::Json<DecideBody>,
) -> Result<HttpResponse, AppError> {
    auth.require_any_role(&["Administrator", "Teacher", "AcademicStaff"])?;

    let (window_id, submission_id) = path.into_inner();

    // Validate decision value.
    if body.decision != "approved" && body.decision != "rejected" {
        return Err(AppError::ValidationError(
            "decision must be 'approved' or 'rejected'.".into(),
        ));
    }

    // Rejection requires a non-empty reason.
    if body.decision == "rejected" {
        match &body.reason {
            None => {
                return Err(AppError::ValidationError(
                    "A reason is required when rejecting a submission.".into(),
                ))
            }
            Some(r) if r.trim().is_empty() => {
                return Err(AppError::ValidationError(
                    "Denial reason must not be empty.".into(),
                ))
            }
            _ => {}
        }
    }

    // Load submission; verify it belongs to this window.
    #[derive(sqlx::FromRow)]
    struct SubmissionMeta {
        student_id: Uuid,
        window_id: Uuid,
    }

    let sub = sqlx::query_as::<_, SubmissionMeta>(
        "SELECT student_id, window_id FROM checkin_submissions WHERE id = $1",
    )
    .bind(submission_id)
    .fetch_optional(pool.get_ref())
    .await?
    .ok_or_else(|| AppError::NotFound("Submission not found".into()))?;

    if sub.window_id != window_id {
        return Err(AppError::ValidationError(
            "Submission does not belong to this window.".into(),
        ));
    }

    // School-scope check.
    let school_id = sqlx::query_scalar::<_, Uuid>(
        "SELECT school_id FROM checkin_windows WHERE id = $1",
    )
    .bind(window_id)
    .fetch_one(pool.get_ref())
    .await?;

    if !auth.is_admin() {
        crate::middleware::auth::require_school_access(&auth, pool.get_ref(), school_id).await?;
    }

    // Fetch existing decision row.
    #[derive(sqlx::FromRow)]
    struct ExistingDecision {
        id: Uuid,
        decision: String,
    }

    let existing = sqlx::query_as::<_, ExistingDecision>(
        "SELECT id, decision FROM checkin_approval_decisions WHERE submission_id = $1",
    )
    .bind(submission_id)
    .fetch_optional(pool.get_ref())
    .await?;

    let old_decision = match &existing {
        Some(ed) if ed.decision != "pending" => {
            return Err(AppError::ConflictError(format!(
                "This submission has already been {}.",
                ed.decision
            )));
        }
        Some(ed) => ed.decision.clone(),
        None => "none".to_string(),
    };

    let now = Utc::now();

    // Upsert decision.
    if let Some(ed) = &existing {
        sqlx::query(
            "UPDATE checkin_approval_decisions
             SET decision = $1, reason = $2, decided_by = $3, decided_at = $4
             WHERE id = $5",
        )
        .bind(&body.decision)
        .bind(body.reason.as_deref())
        .bind(auth.0.user_id)
        .bind(now)
        .bind(ed.id)
        .execute(pool.get_ref())
        .await?;
    } else {
        sqlx::query(
            "INSERT INTO checkin_approval_decisions
             (id, submission_id, decided_by, decision, reason, decided_at)
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(Uuid::new_v4())
        .bind(submission_id)
        .bind(auth.0.user_id)
        .bind(&body.decision)
        .bind(body.reason.as_deref())
        .bind(now)
        .execute(pool.get_ref())
        .await?;
    }

    // Fetch window title for notifications.
    let window_title: String = sqlx::query_scalar("SELECT title FROM checkin_windows WHERE id = $1")
        .bind(window_id)
        .fetch_one(pool.get_ref())
        .await?;

    // Build notification content.
    let (subject, notif_body) = if body.decision == "approved" {
        (
            format!("Check-in approved: {}", window_title),
            format!("Your check-in for '{}' has been approved.", window_title),
        )
    } else {
        let reason_str = body.reason.as_deref().unwrap_or("No reason provided");
        (
            format!("Check-in denied: {}", window_title),
            format!(
                "Your check-in for '{}' was denied. Reason: {}",
                window_title, reason_str
            ),
        )
    };

    // Notify the student.
    create_user_notification(
        pool.get_ref(),
        sub.student_id,
        Some(auth.0.user_id),
        &subject,
        &notif_body,
        "checkin",
    )
    .await?;

    // Notify all linked parents.
    let parent_ids: Vec<Uuid> = sqlx::query_scalar(
        "SELECT parent_id FROM parent_student_links WHERE student_id = $1",
    )
    .bind(sub.student_id)
    .fetch_all(pool.get_ref())
    .await?;

    for parent_id in parent_ids {
        create_user_notification(
            pool.get_ref(),
            parent_id,
            Some(auth.0.user_id),
            &subject,
            &notif_body,
            "checkin",
        )
        .await?;
    }

    // Audit log.
    log_audit(
        pool.get_ref(),
        auth.0.user_id,
        "checkin_decision",
        "checkin_submission",
        &submission_id.to_string(),
        Some(serde_json::json!({ "decision": old_decision })),
        Some(serde_json::json!({
            "decision": body.decision,
            "reason": body.reason,
        })),
    )
    .await?;

    log::info!(
        "checkin decision: submission={} decision={} by={}",
        submission_id,
        body.decision,
        auth.0.user_id
    );

    Ok(HttpResponse::Ok().json(serde_json::json!({
        "submission_id": submission_id,
        "decision": body.decision,
        "decided_by": auth.0.user_id,
        "decided_at": now,
    })))
}

/// GET /api/v1/check-ins/my
///
/// Student: their own submission history.
/// Parent:  submission history for all linked students.
async fn my_checkins(
    pool: web::Data<DbPool>,
    auth: AuthContext,
) -> Result<HttpResponse, AppError> {
    auth.require_any_role(&["Student", "Parent"])?;

    let is_parent = auth.0.roles.iter().any(|r| r == "Parent");

    let rows: Vec<MyCheckinRow> = if is_parent {
        sqlx::query_as::<_, MyCheckinRow>(
            "SELECT
               cs.id                               AS submission_id,
               cs.window_id,
               cw.title                            AS window_title,
               cw.opens_at,
               cw.closes_at,
               cs.submitted_at,
               cs.is_late,
               cs.method,
               COALESCE(cad.decision, 'pending')   AS decision,
               cad.reason,
               cad.decided_at,
               u.username                          AS student_username
             FROM checkin_submissions cs
             JOIN checkin_windows cw ON cs.window_id = cw.id
             LEFT JOIN checkin_approval_decisions cad ON cad.submission_id = cs.id
             JOIN users u ON cs.student_id = u.id
             WHERE cs.student_id IN (
               SELECT student_id FROM parent_student_links WHERE parent_id = $1
             )
             ORDER BY cs.submitted_at DESC
             LIMIT 100",
        )
        .bind(auth.0.user_id)
        .fetch_all(pool.get_ref())
        .await?
    } else {
        sqlx::query_as::<_, MyCheckinRow>(
            "SELECT
               cs.id                               AS submission_id,
               cs.window_id,
               cw.title                            AS window_title,
               cw.opens_at,
               cw.closes_at,
               cs.submitted_at,
               cs.is_late,
               cs.method,
               COALESCE(cad.decision, 'pending')   AS decision,
               cad.reason,
               cad.decided_at,
               u.username                          AS student_username
             FROM checkin_submissions cs
             JOIN checkin_windows cw ON cs.window_id = cw.id
             LEFT JOIN checkin_approval_decisions cad ON cad.submission_id = cs.id
             JOIN users u ON cs.student_id = u.id
             WHERE cs.student_id = $1
             ORDER BY cs.submitted_at DESC
             LIMIT 100",
        )
        .bind(auth.0.user_id)
        .fetch_all(pool.get_ref())
        .await?
    };

    Ok(HttpResponse::Ok().json(rows))
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

/// Verify a student has access to a school — either via a direct
/// user_school_assignment or via an active class enrolment at that school.
async fn require_student_school_access(
    pool: &DbPool,
    student_id: Uuid,
    school_id: Uuid,
) -> Result<(), AppError> {
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM (
           SELECT 1 FROM user_school_assignments
           WHERE user_id = $1 AND school_id = $2
           UNION ALL
           SELECT 1 FROM class_enrollments ce
           JOIN classes c ON ce.class_id = c.id
           WHERE ce.student_id = $1 AND c.school_id = $2 AND ce.status = 'active'
         ) sub",
    )
    .bind(student_id)
    .bind(school_id)
    .fetch_one(pool)
    .await?;

    if count == 0 {
        return Err(AppError::Forbidden(
            "Student is not enrolled at this school.".into(),
        ));
    }
    Ok(())
}

/// Unified view-access check used by list_windows and get_window.
/// Admins pass unconditionally. Teachers/Staff use school assignment.
/// Students and Parents use enrolment / parent link.
async fn require_checkin_view_access(
    auth: &AuthContext,
    pool: &DbPool,
    school_id: Uuid,
) -> Result<(), AppError> {
    if auth.is_admin() {
        return Ok(());
    }

    let is_teacher_or_staff = auth.is_teacher()
        || auth.0.roles.iter().any(|r| r == "AcademicStaff");

    if is_teacher_or_staff {
        return crate::middleware::auth::require_school_access(auth, pool, school_id).await;
    }

    let user_id = auth.0.user_id;
    let is_parent = auth.0.roles.iter().any(|r| r == "Parent");

    if is_parent {
        // Any linked student enrolled at this school suffices.
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM (
               SELECT 1 FROM user_school_assignments
               WHERE user_id = $1 AND school_id = $2
               UNION ALL
               SELECT 1 FROM parent_student_links psl
               JOIN class_enrollments ce ON ce.student_id = psl.student_id
               JOIN classes c ON ce.class_id = c.id
               WHERE psl.parent_id = $1 AND c.school_id = $2 AND ce.status = 'active'
             ) sub",
        )
        .bind(user_id)
        .bind(school_id)
        .fetch_one(pool)
        .await?;

        if count == 0 {
            return Err(AppError::Forbidden(
                "None of your linked students are enrolled at this school.".into(),
            ));
        }
        Ok(())
    } else {
        // Student
        require_student_school_access(pool, user_id, school_id).await
    }
}

async fn log_audit(
    pool: &DbPool,
    actor_id: Uuid,
    action: &str,
    entity_type: &str,
    entity_id: &str,
    old_data: Option<serde_json::Value>,
    new_data: Option<serde_json::Value>,
) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO audit_logs
         (id, actor_id, action, entity_type, entity_id, old_data, new_data, created_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, NOW())",
    )
    .bind(Uuid::new_v4())
    .bind(actor_id)
    .bind(action)
    .bind(entity_type)
    .bind(entity_id)
    .bind(old_data)
    .bind(new_data)
    .execute(pool)
    .await?;
    Ok(())
}

fn extract_ip(req: &HttpRequest) -> Option<String> {
    req.headers()
        .get("X-Forwarded-For")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.split(',').next().unwrap_or(s).trim().to_string())
        .or_else(|| req.peer_addr().map(|addr| addr.ip().to_string()))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::test::{call_service, init_service, read_body_json, TestRequest};
    use actix_web::{web, App};
    use chrono::Duration;
    use serde_json::{json, Value};
    use sqlx::postgres::PgPoolOptions;
    use sqlx::PgPool;

    // ── Helpers ──────────────────────────────────────────────────────────────

    async fn test_pool() -> PgPool {
        let url = std::env::var("DATABASE_URL")
            .expect("DATABASE_URL must be set to run integration tests");
        let pool = PgPoolOptions::new()
            .max_connections(3)
            .connect(&url)
            .await
            .expect("Failed to connect to test database");
        sqlx::migrate!("../migrations")
            .run(&pool)
            .await
            .expect("Failed to run migrations");
        pool
    }

    /// Create a user and assign the given role. Returns the user's UUID.
    async fn seed_user(pool: &PgPool, username: &str, role: &str) -> Uuid {
        let hash =
            crate::services::auth::hash_password("TestPass2024!!").expect("hash failed");
        sqlx::query(
            "INSERT INTO users (id, username, email, password_hash, account_state, created_at, updated_at)
             VALUES (gen_random_uuid(), $1, $2, $3, 'active', NOW(), NOW())
             ON CONFLICT (username) DO UPDATE
               SET password_hash = EXCLUDED.password_hash,
                   account_state = 'active',
                   updated_at = NOW()",
        )
        .bind(username)
        .bind(format!("{}@test.local", username))
        .bind(&hash)
        .execute(pool)
        .await
        .expect("seed_user insert failed");

        let user_id: Uuid = sqlx::query_scalar("SELECT id FROM users WHERE username = $1")
            .bind(username)
            .fetch_one(pool)
            .await
            .expect("seed_user fetch failed");

        let role_id: i32 = sqlx::query_scalar("SELECT id FROM roles WHERE name = $1")
            .bind(role)
            .fetch_one(pool)
            .await
            .expect("role not found — did migrations run?");

        sqlx::query(
            "INSERT INTO user_roles (user_id, role_id) VALUES ($1, $2) ON CONFLICT DO NOTHING",
        )
        .bind(user_id)
        .bind(role_id)
        .execute(pool)
        .await
        .expect("user_roles insert failed");

        user_id
    }

    /// Create district → campus → school. Returns school UUID.
    async fn seed_school(pool: &PgPool, suffix: &str) -> Uuid {
        let district_id = Uuid::new_v4();
        let campus_id = Uuid::new_v4();
        let school_id = Uuid::new_v4();

        sqlx::query(
            "INSERT INTO districts (id, name, state, created_at)
             VALUES ($1, $2, 'TX', NOW())",
        )
        .bind(district_id)
        .bind(format!("Test District {}", suffix))
        .execute(pool)
        .await
        .expect("seed district failed");

        sqlx::query(
            "INSERT INTO campuses (id, district_id, name, created_at)
             VALUES ($1, $2, $3, NOW())",
        )
        .bind(campus_id)
        .bind(district_id)
        .bind(format!("Test Campus {}", suffix))
        .execute(pool)
        .await
        .expect("seed campus failed");

        sqlx::query(
            "INSERT INTO schools (id, campus_id, name, school_type, created_at)
             VALUES ($1, $2, $3, 'general', NOW())",
        )
        .bind(school_id)
        .bind(campus_id)
        .bind(format!("Test School {}", suffix))
        .execute(pool)
        .await
        .expect("seed school failed");

        school_id
    }

    /// Assign a user to a school in user_school_assignments.
    async fn seed_school_assignment(pool: &PgPool, user_id: Uuid, school_id: Uuid) {
        sqlx::query(
            "INSERT INTO user_school_assignments (user_id, school_id, assignment_type, assigned_at)
             VALUES ($1, $2, 'staff', NOW())
             ON CONFLICT DO NOTHING",
        )
        .bind(user_id)
        .bind(school_id)
        .execute(pool)
        .await
        .expect("seed school assignment failed");
    }

    /// Create a class at a school and enrol a student. Returns class UUID.
    async fn seed_class_with_student(
        pool: &PgPool,
        school_id: Uuid,
        teacher_id: Uuid,
        student_id: Uuid,
        suffix: &str,
    ) -> Uuid {
        let class_id = Uuid::new_v4();

        sqlx::query(
            "INSERT INTO classes (id, school_id, teacher_id, name, academic_year, active, created_at)
             VALUES ($1, $2, $3, $4, '2025', TRUE, NOW())",
        )
        .bind(class_id)
        .bind(school_id)
        .bind(teacher_id)
        .bind(format!("Test Class {}", suffix))
        .execute(pool)
        .await
        .expect("seed class failed");

        sqlx::query(
            "INSERT INTO class_enrollments (class_id, student_id, status, enrolled_at)
             VALUES ($1, $2, 'active', NOW())
             ON CONFLICT DO NOTHING",
        )
        .bind(class_id)
        .bind(student_id)
        .execute(pool)
        .await
        .expect("seed enrollment failed");

        class_id
    }

    /// Create a parent–student link.
    async fn seed_parent_link(pool: &PgPool, parent_id: Uuid, student_id: Uuid) {
        sqlx::query(
            "INSERT INTO parent_student_links (parent_id, student_id, relationship)
             VALUES ($1, $2, 'parent')
             ON CONFLICT DO NOTHING",
        )
        .bind(parent_id)
        .bind(student_id)
        .execute(pool)
        .await
        .expect("seed parent link failed");
    }

    /// Create a check-in window. Returns window UUID.
    async fn seed_window(
        pool: &PgPool,
        school_id: Uuid,
        opens_at: DateTime<Utc>,
        closes_at: DateTime<Utc>,
        allow_late: bool,
        active: bool,
        created_by: Uuid,
    ) -> Uuid {
        let window_id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO checkin_windows
             (id, school_id, title, opens_at, closes_at, allow_late, active, created_by)
             VALUES ($1, $2, 'Test Window', $3, $4, $5, $6, $7)",
        )
        .bind(window_id)
        .bind(school_id)
        .bind(opens_at)
        .bind(closes_at)
        .bind(allow_late)
        .bind(active)
        .bind(created_by)
        .execute(pool)
        .await
        .expect("seed window failed");
        window_id
    }

    /// Log in and return the bearer token.
    async fn login_as(
        app: &impl actix_web::dev::Service<
            actix_http::Request,
            Response = actix_web::dev::ServiceResponse,
            Error = actix_web::Error,
        >,
        username: &str,
    ) -> String {
        let req = TestRequest::post()
            .uri("/api/v1/auth/login")
            .set_json(json!({ "username": username, "password": "TestPass2024!!" }))
            .to_request();
        let resp = call_service(app, req).await;
        let body: Value = read_body_json(resp).await;
        body["token"]
            .as_str()
            .expect("login did not return token")
            .to_string()
    }

    // ── Tests ─────────────────────────────────────────────────────────────────

    /// Happy path: student submits for an open window they are enrolled for → 201.
    #[actix_web::test]
    #[ignore = "requires DATABASE_URL"]
    async fn test_submit_checkin_success() {
        let pool = test_pool().await;
        let suffix = Uuid::new_v4().to_string()[..8].to_string();

        let admin_id = seed_user(&pool, &format!("ci_admin_{}", suffix), "Administrator").await;
        let teacher_id = seed_user(&pool, &format!("ci_teacher_{}", suffix), "Teacher").await;
        let student_id = seed_user(&pool, &format!("ci_student_{}", suffix), "Student").await;

        let school_id = seed_school(&pool, &suffix).await;
        seed_school_assignment(&pool, teacher_id, school_id).await;
        seed_class_with_student(&pool, school_id, teacher_id, student_id, &suffix).await;

        let opens = Utc::now() - Duration::hours(1);
        let closes = Utc::now() + Duration::hours(1);
        let window_id =
            seed_window(&pool, school_id, opens, closes, false, true, admin_id).await;

        let app = init_service(
            App::new()
                .app_data(web::Data::new(pool.clone()))
                .configure(crate::routes::configure_routes),
        )
        .await;

        let token = login_as(&app, &format!("ci_student_{}", suffix)).await;
        let req = TestRequest::post()
            .uri(&format!("/api/v1/check-ins/windows/{}/submit", window_id))
            .insert_header(("Authorization", format!("Bearer {}", token)))
            .set_json(json!({ "notes": "present" }))
            .to_request();
        let resp = call_service(&app, req).await;

        assert_eq!(resp.status(), 201, "expected 201 Created");
        let body: Value = read_body_json(resp).await;
        assert_eq!(body["status"], "pending");
        assert_eq!(body["student_id"].as_str().unwrap(), student_id.to_string());

        // Verify DB row exists.
        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM checkin_submissions WHERE window_id = $1")
                .bind(window_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(count, 1, "expected exactly one submission row");
    }

    /// Duplicate submission for same (window, student) → 409.
    #[actix_web::test]
    #[ignore = "requires DATABASE_URL"]
    async fn test_submit_checkin_duplicate() {
        let pool = test_pool().await;
        let suffix = Uuid::new_v4().to_string()[..8].to_string();

        let admin_id = seed_user(&pool, &format!("dup_admin_{}", suffix), "Administrator").await;
        let teacher_id = seed_user(&pool, &format!("dup_teacher_{}", suffix), "Teacher").await;
        let student_id = seed_user(&pool, &format!("dup_student_{}", suffix), "Student").await;

        let school_id = seed_school(&pool, &format!("dup_{}", suffix)).await;
        seed_school_assignment(&pool, teacher_id, school_id).await;
        seed_class_with_student(&pool, school_id, teacher_id, student_id, &suffix).await;

        let opens = Utc::now() - Duration::hours(1);
        let closes = Utc::now() + Duration::hours(1);
        let window_id =
            seed_window(&pool, school_id, opens, closes, false, true, admin_id).await;

        let app = init_service(
            App::new()
                .app_data(web::Data::new(pool.clone()))
                .configure(crate::routes::configure_routes),
        )
        .await;

        let token = login_as(&app, &format!("dup_student_{}", suffix)).await;
        let submit_path = format!("/api/v1/check-ins/windows/{}/submit", window_id);

        // First submission → 201.
        let req = TestRequest::post()
            .uri(&submit_path)
            .insert_header(("Authorization", format!("Bearer {}", token)))
            .set_json(json!({}))
            .to_request();
        let resp = call_service(&app, req).await;
        assert_eq!(resp.status(), 201);

        // Second submission → 409.
        let req = TestRequest::post()
            .uri(&submit_path)
            .insert_header(("Authorization", format!("Bearer {}", token)))
            .set_json(json!({}))
            .to_request();
        let resp = call_service(&app, req).await;
        assert_eq!(resp.status(), 409, "expected 409 on duplicate submission");
    }

    /// Window closed (closes_at in the past, allow_late=false) → 422.
    #[actix_web::test]
    #[ignore = "requires DATABASE_URL"]
    async fn test_submit_checkin_closed_window() {
        let pool = test_pool().await;
        let suffix = Uuid::new_v4().to_string()[..8].to_string();

        let admin_id = seed_user(&pool, &format!("cl_admin_{}", suffix), "Administrator").await;
        let teacher_id = seed_user(&pool, &format!("cl_teacher_{}", suffix), "Teacher").await;
        let student_id = seed_user(&pool, &format!("cl_student_{}", suffix), "Student").await;

        let school_id = seed_school(&pool, &format!("cl_{}", suffix)).await;
        seed_school_assignment(&pool, teacher_id, school_id).await;
        seed_class_with_student(&pool, school_id, teacher_id, student_id, &suffix).await;

        let opens = Utc::now() - Duration::hours(3);
        let closes = Utc::now() - Duration::hours(1); // already closed
        let window_id =
            seed_window(&pool, school_id, opens, closes, false, true, admin_id).await;

        let app = init_service(
            App::new()
                .app_data(web::Data::new(pool.clone()))
                .configure(crate::routes::configure_routes),
        )
        .await;

        let token = login_as(&app, &format!("cl_student_{}", suffix)).await;
        let req = TestRequest::post()
            .uri(&format!("/api/v1/check-ins/windows/{}/submit", window_id))
            .insert_header(("Authorization", format!("Bearer {}", token)))
            .set_json(json!({}))
            .to_request();
        let resp = call_service(&app, req).await;
        assert_eq!(resp.status(), 422, "expected 422 for closed window");
    }

    /// Window closed but allow_late=true → 201 with is_late=true.
    #[actix_web::test]
    #[ignore = "requires DATABASE_URL"]
    async fn test_submit_checkin_allow_late() {
        let pool = test_pool().await;
        let suffix = Uuid::new_v4().to_string()[..8].to_string();

        let admin_id = seed_user(&pool, &format!("lt_admin_{}", suffix), "Administrator").await;
        let teacher_id = seed_user(&pool, &format!("lt_teacher_{}", suffix), "Teacher").await;
        let student_id = seed_user(&pool, &format!("lt_student_{}", suffix), "Student").await;

        let school_id = seed_school(&pool, &format!("lt_{}", suffix)).await;
        seed_school_assignment(&pool, teacher_id, school_id).await;
        seed_class_with_student(&pool, school_id, teacher_id, student_id, &suffix).await;

        let opens = Utc::now() - Duration::hours(3);
        let closes = Utc::now() - Duration::hours(1);
        // allow_late = true
        let window_id =
            seed_window(&pool, school_id, opens, closes, true, true, admin_id).await;

        let app = init_service(
            App::new()
                .app_data(web::Data::new(pool.clone()))
                .configure(crate::routes::configure_routes),
        )
        .await;

        let token = login_as(&app, &format!("lt_student_{}", suffix)).await;
        let req = TestRequest::post()
            .uri(&format!("/api/v1/check-ins/windows/{}/submit", window_id))
            .insert_header(("Authorization", format!("Bearer {}", token)))
            .set_json(json!({}))
            .to_request();
        let resp = call_service(&app, req).await;
        assert_eq!(resp.status(), 201, "expected 201 when allow_late=true");
        let body: Value = read_body_json(resp).await;
        assert_eq!(body["is_late"], true, "expected is_late=true");
    }

    /// Window active=false → 422.
    #[actix_web::test]
    #[ignore = "requires DATABASE_URL"]
    async fn test_submit_checkin_inactive_window() {
        let pool = test_pool().await;
        let suffix = Uuid::new_v4().to_string()[..8].to_string();

        let admin_id = seed_user(&pool, &format!("ia_admin_{}", suffix), "Administrator").await;
        let teacher_id = seed_user(&pool, &format!("ia_teacher_{}", suffix), "Teacher").await;
        let student_id = seed_user(&pool, &format!("ia_student_{}", suffix), "Student").await;

        let school_id = seed_school(&pool, &format!("ia_{}", suffix)).await;
        seed_school_assignment(&pool, teacher_id, school_id).await;
        seed_class_with_student(&pool, school_id, teacher_id, student_id, &suffix).await;

        let opens = Utc::now() - Duration::hours(1);
        let closes = Utc::now() + Duration::hours(1);
        // active = false
        let window_id =
            seed_window(&pool, school_id, opens, closes, false, false, admin_id).await;

        let app = init_service(
            App::new()
                .app_data(web::Data::new(pool.clone()))
                .configure(crate::routes::configure_routes),
        )
        .await;

        let token = login_as(&app, &format!("ia_student_{}", suffix)).await;
        let req = TestRequest::post()
            .uri(&format!("/api/v1/check-ins/windows/{}/submit", window_id))
            .insert_header(("Authorization", format!("Bearer {}", token)))
            .set_json(json!({}))
            .to_request();
        let resp = call_service(&app, req).await;
        assert_eq!(resp.status(), 422, "expected 422 for inactive window");
    }

    /// Teacher role calling submit → 403.
    #[actix_web::test]
    #[ignore = "requires DATABASE_URL"]
    async fn test_submit_checkin_wrong_role() {
        let pool = test_pool().await;
        let suffix = Uuid::new_v4().to_string()[..8].to_string();

        let admin_id = seed_user(&pool, &format!("wr_admin_{}", suffix), "Administrator").await;
        let teacher_id = seed_user(&pool, &format!("wr_teacher_{}", suffix), "Teacher").await;

        let school_id = seed_school(&pool, &format!("wr_{}", suffix)).await;
        seed_school_assignment(&pool, teacher_id, school_id).await;

        let opens = Utc::now() - Duration::hours(1);
        let closes = Utc::now() + Duration::hours(1);
        let window_id =
            seed_window(&pool, school_id, opens, closes, false, true, admin_id).await;

        let app = init_service(
            App::new()
                .app_data(web::Data::new(pool.clone()))
                .configure(crate::routes::configure_routes),
        )
        .await;

        let token = login_as(&app, &format!("wr_teacher_{}", suffix)).await;
        let req = TestRequest::post()
            .uri(&format!("/api/v1/check-ins/windows/{}/submit", window_id))
            .insert_header(("Authorization", format!("Bearer {}", token)))
            .set_json(json!({}))
            .to_request();
        let resp = call_service(&app, req).await;
        assert_eq!(resp.status(), 403, "teacher should not be allowed to submit");
    }

    /// Parent submits on behalf of linked student → 201 with method='parent'.
    #[actix_web::test]
    #[ignore = "requires DATABASE_URL"]
    async fn test_submit_checkin_parent_success() {
        let pool = test_pool().await;
        let suffix = Uuid::new_v4().to_string()[..8].to_string();

        let admin_id = seed_user(&pool, &format!("ps_admin_{}", suffix), "Administrator").await;
        let teacher_id = seed_user(&pool, &format!("ps_teacher_{}", suffix), "Teacher").await;
        let student_id = seed_user(&pool, &format!("ps_student_{}", suffix), "Student").await;
        let parent_id = seed_user(&pool, &format!("ps_parent_{}", suffix), "Parent").await;

        let school_id = seed_school(&pool, &format!("ps_{}", suffix)).await;
        seed_school_assignment(&pool, teacher_id, school_id).await;
        seed_class_with_student(&pool, school_id, teacher_id, student_id, &suffix).await;
        seed_parent_link(&pool, parent_id, student_id).await;

        let opens = Utc::now() - Duration::hours(1);
        let closes = Utc::now() + Duration::hours(1);
        let window_id =
            seed_window(&pool, school_id, opens, closes, false, true, admin_id).await;

        let app = init_service(
            App::new()
                .app_data(web::Data::new(pool.clone()))
                .configure(crate::routes::configure_routes),
        )
        .await;

        let token = login_as(&app, &format!("ps_parent_{}", suffix)).await;
        let req = TestRequest::post()
            .uri(&format!("/api/v1/check-ins/windows/{}/submit", window_id))
            .insert_header(("Authorization", format!("Bearer {}", token)))
            .set_json(json!({ "student_id": student_id }))
            .to_request();
        let resp = call_service(&app, req).await;
        assert_eq!(resp.status(), 201, "parent should be able to submit for linked student");

        let body: Value = read_body_json(resp).await;
        assert_eq!(
            body["student_id"].as_str().unwrap(),
            student_id.to_string(),
            "student_id in response must be the linked student"
        );
    }

    /// Parent submits for unlinked student → 403.
    #[actix_web::test]
    #[ignore = "requires DATABASE_URL"]
    async fn test_submit_checkin_parent_unlinked() {
        let pool = test_pool().await;
        let suffix = Uuid::new_v4().to_string()[..8].to_string();

        let admin_id = seed_user(&pool, &format!("pu_admin_{}", suffix), "Administrator").await;
        let teacher_id = seed_user(&pool, &format!("pu_teacher_{}", suffix), "Teacher").await;
        let student_id = seed_user(&pool, &format!("pu_student_{}", suffix), "Student").await;
        let parent_id = seed_user(&pool, &format!("pu_parent_{}", suffix), "Parent").await;
        // No parent_student_links row created.

        let school_id = seed_school(&pool, &format!("pu_{}", suffix)).await;
        seed_school_assignment(&pool, teacher_id, school_id).await;
        seed_class_with_student(&pool, school_id, teacher_id, student_id, &suffix).await;

        let opens = Utc::now() - Duration::hours(1);
        let closes = Utc::now() + Duration::hours(1);
        let window_id =
            seed_window(&pool, school_id, opens, closes, false, true, admin_id).await;

        let app = init_service(
            App::new()
                .app_data(web::Data::new(pool.clone()))
                .configure(crate::routes::configure_routes),
        )
        .await;

        let token = login_as(&app, &format!("pu_parent_{}", suffix)).await;
        let req = TestRequest::post()
            .uri(&format!("/api/v1/check-ins/windows/{}/submit", window_id))
            .insert_header(("Authorization", format!("Bearer {}", token)))
            .set_json(json!({ "student_id": student_id }))
            .to_request();
        let resp = call_service(&app, req).await;
        assert_eq!(resp.status(), 403, "parent must not submit for unlinked student");
    }

    /// Teacher reviews submissions for their school → 200 with list.
    #[actix_web::test]
    #[ignore = "requires DATABASE_URL"]
    async fn test_list_submissions_teacher_success() {
        let pool = test_pool().await;
        let suffix = Uuid::new_v4().to_string()[..8].to_string();

        let admin_id = seed_user(&pool, &format!("ls_admin_{}", suffix), "Administrator").await;
        let teacher_id = seed_user(&pool, &format!("ls_teacher_{}", suffix), "Teacher").await;
        let student_id = seed_user(&pool, &format!("ls_student_{}", suffix), "Student").await;

        let school_id = seed_school(&pool, &format!("ls_{}", suffix)).await;
        seed_school_assignment(&pool, teacher_id, school_id).await;
        seed_class_with_student(&pool, school_id, teacher_id, student_id, &suffix).await;

        let opens = Utc::now() - Duration::hours(1);
        let closes = Utc::now() + Duration::hours(1);
        let window_id =
            seed_window(&pool, school_id, opens, closes, false, true, admin_id).await;

        let app = init_service(
            App::new()
                .app_data(web::Data::new(pool.clone()))
                .configure(crate::routes::configure_routes),
        )
        .await;

        // Student submits first.
        let student_token = login_as(&app, &format!("ls_student_{}", suffix)).await;
        let req = TestRequest::post()
            .uri(&format!("/api/v1/check-ins/windows/{}/submit", window_id))
            .insert_header(("Authorization", format!("Bearer {}", student_token)))
            .set_json(json!({}))
            .to_request();
        let resp = call_service(&app, req).await;
        assert_eq!(resp.status(), 201);

        // Teacher lists submissions.
        let teacher_token = login_as(&app, &format!("ls_teacher_{}", suffix)).await;
        let req = TestRequest::get()
            .uri(&format!(
                "/api/v1/check-ins/windows/{}/submissions",
                window_id
            ))
            .insert_header(("Authorization", format!("Bearer {}", teacher_token)))
            .to_request();
        let resp = call_service(&app, req).await;
        assert_eq!(resp.status(), 200);
        let body: Value = read_body_json(resp).await;
        let arr = body.as_array().expect("expected array");
        assert_eq!(arr.len(), 1, "expected one submission");
        assert_eq!(arr[0]["decision"], "pending");
    }

    /// Teacher from different school tries to list submissions → 403.
    #[actix_web::test]
    #[ignore = "requires DATABASE_URL"]
    async fn test_list_submissions_out_of_scope() {
        let pool = test_pool().await;
        let suffix = Uuid::new_v4().to_string()[..8].to_string();

        let admin_id = seed_user(&pool, &format!("oos_admin_{}", suffix), "Administrator").await;
        let teacher_in = seed_user(&pool, &format!("oos_tin_{}", suffix), "Teacher").await;
        let teacher_out = seed_user(&pool, &format!("oos_tout_{}", suffix), "Teacher").await;

        let school_id = seed_school(&pool, &format!("oos_{}", suffix)).await;
        let other_school_id = seed_school(&pool, &format!("oos2_{}", suffix)).await;

        seed_school_assignment(&pool, teacher_in, school_id).await;
        seed_school_assignment(&pool, teacher_out, other_school_id).await;

        let opens = Utc::now() - Duration::hours(1);
        let closes = Utc::now() + Duration::hours(1);
        let window_id =
            seed_window(&pool, school_id, opens, closes, false, true, admin_id).await;

        let app = init_service(
            App::new()
                .app_data(web::Data::new(pool.clone()))
                .configure(crate::routes::configure_routes),
        )
        .await;

        let token = login_as(&app, &format!("oos_tout_{}", suffix)).await;
        let req = TestRequest::get()
            .uri(&format!(
                "/api/v1/check-ins/windows/{}/submissions",
                window_id
            ))
            .insert_header(("Authorization", format!("Bearer {}", token)))
            .to_request();
        let resp = call_service(&app, req).await;
        assert_eq!(
            resp.status(),
            403,
            "teacher from different school should be 403"
        );
    }

    /// Teacher approves a pending submission → 200, DB updated, notification created.
    #[actix_web::test]
    #[ignore = "requires DATABASE_URL"]
    async fn test_decide_approve_success() {
        let pool = test_pool().await;
        let suffix = Uuid::new_v4().to_string()[..8].to_string();

        let admin_id = seed_user(&pool, &format!("da_admin_{}", suffix), "Administrator").await;
        let teacher_id = seed_user(&pool, &format!("da_teacher_{}", suffix), "Teacher").await;
        let student_id = seed_user(&pool, &format!("da_student_{}", suffix), "Student").await;

        let school_id = seed_school(&pool, &format!("da_{}", suffix)).await;
        seed_school_assignment(&pool, teacher_id, school_id).await;
        seed_class_with_student(&pool, school_id, teacher_id, student_id, &suffix).await;

        let opens = Utc::now() - Duration::hours(1);
        let closes = Utc::now() + Duration::hours(1);
        let window_id =
            seed_window(&pool, school_id, opens, closes, false, true, admin_id).await;

        let app = init_service(
            App::new()
                .app_data(web::Data::new(pool.clone()))
                .configure(crate::routes::configure_routes),
        )
        .await;

        // Student submits.
        let student_token = login_as(&app, &format!("da_student_{}", suffix)).await;
        let req = TestRequest::post()
            .uri(&format!("/api/v1/check-ins/windows/{}/submit", window_id))
            .insert_header(("Authorization", format!("Bearer {}", student_token)))
            .set_json(json!({}))
            .to_request();
        let resp = call_service(&app, req).await;
        assert_eq!(resp.status(), 201);
        let body: Value = read_body_json(resp).await;
        let submission_id = body["submission_id"].as_str().unwrap().to_string();

        // Teacher approves.
        let teacher_token = login_as(&app, &format!("da_teacher_{}", suffix)).await;
        let decide_path = format!(
            "/api/v1/check-ins/windows/{}/submissions/{}/decide",
            window_id, submission_id
        );
        let req = TestRequest::post()
            .uri(&decide_path)
            .insert_header(("Authorization", format!("Bearer {}", teacher_token)))
            .set_json(json!({ "decision": "approved" }))
            .to_request();
        let resp = call_service(&app, req).await;
        assert_eq!(resp.status(), 200, "approve should return 200");

        // Verify DB.
        let decision: String = sqlx::query_scalar(
            "SELECT decision FROM checkin_approval_decisions WHERE submission_id = $1",
        )
        .bind(Uuid::parse_str(&submission_id).unwrap())
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(decision, "approved");

        // Verify notification created for student.
        let notif_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM notifications WHERE recipient_id = $1 AND notification_type = 'checkin'",
        )
        .bind(student_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(notif_count >= 1, "expected at least one notification for student");
    }

    /// Reject without reason → 422.
    #[actix_web::test]
    #[ignore = "requires DATABASE_URL"]
    async fn test_decide_reject_no_reason() {
        let pool = test_pool().await;
        let suffix = Uuid::new_v4().to_string()[..8].to_string();

        let admin_id = seed_user(&pool, &format!("dr_admin_{}", suffix), "Administrator").await;
        let teacher_id = seed_user(&pool, &format!("dr_teacher_{}", suffix), "Teacher").await;
        let student_id = seed_user(&pool, &format!("dr_student_{}", suffix), "Student").await;

        let school_id = seed_school(&pool, &format!("dr_{}", suffix)).await;
        seed_school_assignment(&pool, teacher_id, school_id).await;
        seed_class_with_student(&pool, school_id, teacher_id, student_id, &suffix).await;

        let opens = Utc::now() - Duration::hours(1);
        let closes = Utc::now() + Duration::hours(1);
        let window_id =
            seed_window(&pool, school_id, opens, closes, false, true, admin_id).await;

        let app = init_service(
            App::new()
                .app_data(web::Data::new(pool.clone()))
                .configure(crate::routes::configure_routes),
        )
        .await;

        let student_token = login_as(&app, &format!("dr_student_{}", suffix)).await;
        let req = TestRequest::post()
            .uri(&format!("/api/v1/check-ins/windows/{}/submit", window_id))
            .insert_header(("Authorization", format!("Bearer {}", student_token)))
            .set_json(json!({}))
            .to_request();
        let resp = call_service(&app, req).await;
        let submit_body: Value = read_body_json(resp).await;
        let submission_id = submit_body["submission_id"].as_str().unwrap().to_string();

        let teacher_token = login_as(&app, &format!("dr_teacher_{}", suffix)).await;
        let req = TestRequest::post()
            .uri(&format!(
                "/api/v1/check-ins/windows/{}/submissions/{}/decide",
                window_id, submission_id
            ))
            .insert_header(("Authorization", format!("Bearer {}", teacher_token)))
            .set_json(json!({ "decision": "rejected" }))
            .to_request();
        let resp = call_service(&app, req).await;
        assert_eq!(resp.status(), 422, "rejection without reason must be 422");
    }

    /// Re-deciding an already-decided submission → 409.
    #[actix_web::test]
    #[ignore = "requires DATABASE_URL"]
    async fn test_decide_already_decided() {
        let pool = test_pool().await;
        let suffix = Uuid::new_v4().to_string()[..8].to_string();

        let admin_id = seed_user(&pool, &format!("ad_admin_{}", suffix), "Administrator").await;
        let teacher_id = seed_user(&pool, &format!("ad_teacher_{}", suffix), "Teacher").await;
        let student_id = seed_user(&pool, &format!("ad_student_{}", suffix), "Student").await;

        let school_id = seed_school(&pool, &format!("ad_{}", suffix)).await;
        seed_school_assignment(&pool, teacher_id, school_id).await;
        seed_class_with_student(&pool, school_id, teacher_id, student_id, &suffix).await;

        let opens = Utc::now() - Duration::hours(1);
        let closes = Utc::now() + Duration::hours(1);
        let window_id =
            seed_window(&pool, school_id, opens, closes, false, true, admin_id).await;

        let app = init_service(
            App::new()
                .app_data(web::Data::new(pool.clone()))
                .configure(crate::routes::configure_routes),
        )
        .await;

        let student_token = login_as(&app, &format!("ad_student_{}", suffix)).await;
        let req = TestRequest::post()
            .uri(&format!("/api/v1/check-ins/windows/{}/submit", window_id))
            .insert_header(("Authorization", format!("Bearer {}", student_token)))
            .set_json(json!({}))
            .to_request();
        let resp = call_service(&app, req).await;
        let body: Value = read_body_json(resp).await;
        let submission_id = body["submission_id"].as_str().unwrap().to_string();

        let teacher_token = login_as(&app, &format!("ad_teacher_{}", suffix)).await;
        let decide_path = format!(
            "/api/v1/check-ins/windows/{}/submissions/{}/decide",
            window_id, submission_id
        );

        // First decision → 200.
        let req = TestRequest::post()
            .uri(&decide_path)
            .insert_header(("Authorization", format!("Bearer {}", teacher_token)))
            .set_json(json!({ "decision": "approved" }))
            .to_request();
        let resp = call_service(&app, req).await;
        assert_eq!(resp.status(), 200);

        // Second decision → 409.
        let req = TestRequest::post()
            .uri(&decide_path)
            .insert_header(("Authorization", format!("Bearer {}", teacher_token)))
            .set_json(json!({ "decision": "approved" }))
            .to_request();
        let resp = call_service(&app, req).await;
        assert_eq!(resp.status(), 409, "re-deciding must return 409");
    }

    /// Invalid decision value → 422.
    #[actix_web::test]
    #[ignore = "requires DATABASE_URL"]
    async fn test_decide_invalid_value() {
        let pool = test_pool().await;
        let suffix = Uuid::new_v4().to_string()[..8].to_string();

        let admin_id = seed_user(&pool, &format!("iv_admin_{}", suffix), "Administrator").await;
        let teacher_id = seed_user(&pool, &format!("iv_teacher_{}", suffix), "Teacher").await;
        let student_id = seed_user(&pool, &format!("iv_student_{}", suffix), "Student").await;

        let school_id = seed_school(&pool, &format!("iv_{}", suffix)).await;
        seed_school_assignment(&pool, teacher_id, school_id).await;
        seed_class_with_student(&pool, school_id, teacher_id, student_id, &suffix).await;

        let opens = Utc::now() - Duration::hours(1);
        let closes = Utc::now() + Duration::hours(1);
        let window_id =
            seed_window(&pool, school_id, opens, closes, false, true, admin_id).await;

        let app = init_service(
            App::new()
                .app_data(web::Data::new(pool.clone()))
                .configure(crate::routes::configure_routes),
        )
        .await;

        let student_token = login_as(&app, &format!("iv_student_{}", suffix)).await;
        let req = TestRequest::post()
            .uri(&format!("/api/v1/check-ins/windows/{}/submit", window_id))
            .insert_header(("Authorization", format!("Bearer {}", student_token)))
            .set_json(json!({}))
            .to_request();
        let resp = call_service(&app, req).await;
        let body: Value = read_body_json(resp).await;
        let submission_id = body["submission_id"].as_str().unwrap().to_string();

        let teacher_token = login_as(&app, &format!("iv_teacher_{}", suffix)).await;
        let req = TestRequest::post()
            .uri(&format!(
                "/api/v1/check-ins/windows/{}/submissions/{}/decide",
                window_id, submission_id
            ))
            .insert_header(("Authorization", format!("Bearer {}", teacher_token)))
            .set_json(json!({ "decision": "maybe" }))
            .to_request();
        let resp = call_service(&app, req).await;
        assert_eq!(resp.status(), 422, "invalid decision value must return 422");
    }

    /// Reviewer scope: teacher not assigned to window's school → 403 on decide.
    #[actix_web::test]
    #[ignore = "requires DATABASE_URL"]
    async fn test_decide_reviewer_out_of_scope() {
        let pool = test_pool().await;
        let suffix = Uuid::new_v4().to_string()[..8].to_string();

        let admin_id = seed_user(&pool, &format!("rs_admin_{}", suffix), "Administrator").await;
        let teacher_in = seed_user(&pool, &format!("rs_tin_{}", suffix), "Teacher").await;
        let teacher_out = seed_user(&pool, &format!("rs_tout_{}", suffix), "Teacher").await;
        let student_id = seed_user(&pool, &format!("rs_student_{}", suffix), "Student").await;

        let school_id = seed_school(&pool, &format!("rs_{}", suffix)).await;
        let other_school_id = seed_school(&pool, &format!("rs2_{}", suffix)).await;
        seed_school_assignment(&pool, teacher_in, school_id).await;
        seed_school_assignment(&pool, teacher_out, other_school_id).await;
        seed_class_with_student(&pool, school_id, teacher_in, student_id, &suffix).await;

        let opens = Utc::now() - Duration::hours(1);
        let closes = Utc::now() + Duration::hours(1);
        let window_id =
            seed_window(&pool, school_id, opens, closes, false, true, admin_id).await;

        let app = init_service(
            App::new()
                .app_data(web::Data::new(pool.clone()))
                .configure(crate::routes::configure_routes),
        )
        .await;

        // Student submits.
        let student_token = login_as(&app, &format!("rs_student_{}", suffix)).await;
        let req = TestRequest::post()
            .uri(&format!("/api/v1/check-ins/windows/{}/submit", window_id))
            .insert_header(("Authorization", format!("Bearer {}", student_token)))
            .set_json(json!({}))
            .to_request();
        let resp = call_service(&app, req).await;
        let body: Value = read_body_json(resp).await;
        let submission_id = body["submission_id"].as_str().unwrap().to_string();

        // Out-of-scope teacher tries to decide → 403.
        let token = login_as(&app, &format!("rs_tout_{}", suffix)).await;
        let req = TestRequest::post()
            .uri(&format!(
                "/api/v1/check-ins/windows/{}/submissions/{}/decide",
                window_id, submission_id
            ))
            .insert_header(("Authorization", format!("Bearer {}", token)))
            .set_json(json!({ "decision": "approved" }))
            .to_request();
        let resp = call_service(&app, req).await;
        assert_eq!(
            resp.status(),
            403,
            "out-of-scope teacher should be 403 on decide"
        );
    }

    // ── Submission filter tests ───────────────────────────────────────────────

    /// decision filter `pending` only returns un-decided submissions.
    #[actix_web::test]
    #[ignore = "requires DATABASE_URL"]
    async fn test_filter_decision_pending_excludes_approved() {
        let pool = test_pool().await;
        let suffix = Uuid::new_v4().to_string()[..8].to_string();

        let admin_id = seed_user(&pool, &format!("fp_admin_{}", suffix), "Administrator").await;
        let teacher_id = seed_user(&pool, &format!("fp_teacher_{}", suffix), "Teacher").await;
        let s1 = seed_user(&pool, &format!("fp_s1_{}", suffix), "Student").await;
        let s2 = seed_user(&pool, &format!("fp_s2_{}", suffix), "Student").await;

        let school_id = seed_school(&pool, &format!("fp_{}", suffix)).await;
        seed_school_assignment(&pool, teacher_id, school_id).await;
        seed_class_with_student(&pool, school_id, teacher_id, s1, &format!("fp1_{}", suffix)).await;
        seed_class_with_student(&pool, school_id, teacher_id, s2, &format!("fp2_{}", suffix)).await;

        let opens = Utc::now() - Duration::hours(2);
        let closes = Utc::now() + Duration::hours(2);
        let window_id = seed_window(&pool, school_id, opens, closes, false, true, admin_id).await;

        // s1 submits and gets approved; s2 submits and stays pending.
        let sub1 = Uuid::new_v4();
        let sub2 = Uuid::new_v4();
        for (sub_id, student_id) in [(sub1, s1), (sub2, s2)] {
            sqlx::query(
                "INSERT INTO checkin_submissions (id, window_id, student_id, submitted_at, method, is_late)
                 VALUES ($1, $2, $3, NOW(), 'manual', FALSE)",
            )
            .bind(sub_id)
            .bind(window_id)
            .bind(student_id)
            .execute(&pool)
            .await
            .unwrap();
        }
        // Approve s1's submission.
        sqlx::query(
            "INSERT INTO checkin_approval_decisions (id, submission_id, decided_by, decision, decided_at)
             VALUES (gen_random_uuid(), $1, $2, 'approved', NOW())",
        )
        .bind(sub1)
        .bind(admin_id)
        .execute(&pool)
        .await
        .unwrap();

        let app = init_service(
            App::new()
                .app_data(web::Data::new(pool.clone()))
                .configure(crate::routes::configure_routes),
        )
        .await;
        let token = login_as(&app, &format!("fp_teacher_{}", suffix)).await;

        let req = TestRequest::get()
            .uri(&format!(
                "/api/v1/check-ins/windows/{}/submissions?decision=pending",
                window_id
            ))
            .insert_header(("Authorization", format!("Bearer {}", token)))
            .to_request();
        let resp = call_service(&app, req).await;
        assert_eq!(resp.status(), 200);
        let body: Value = read_body_json(resp).await;
        let subs = body.as_array().unwrap();
        assert_eq!(subs.len(), 1, "only s2's pending submission should appear");
        assert_eq!(subs[0]["decision"], "pending");
    }

    /// homeroom_id filter only returns students enrolled in that homeroom.
    #[actix_web::test]
    #[ignore = "requires DATABASE_URL"]
    async fn test_filter_homeroom_excludes_other_class() {
        let pool = test_pool().await;
        let suffix = Uuid::new_v4().to_string()[..8].to_string();

        let admin_id = seed_user(&pool, &format!("fh_admin_{}", suffix), "Administrator").await;
        let teacher_id = seed_user(&pool, &format!("fh_teacher_{}", suffix), "Teacher").await;
        let s_a = seed_user(&pool, &format!("fh_sa_{}", suffix), "Student").await;
        let s_b = seed_user(&pool, &format!("fh_sb_{}", suffix), "Student").await;

        let school_id = seed_school(&pool, &format!("fh_{}", suffix)).await;
        seed_school_assignment(&pool, teacher_id, school_id).await;
        let class_a = seed_class_with_student(&pool, school_id, teacher_id, s_a, &format!("fhA_{}", suffix)).await;
        seed_class_with_student(&pool, school_id, teacher_id, s_b, &format!("fhB_{}", suffix)).await;

        let opens = Utc::now() - Duration::hours(2);
        let closes = Utc::now() + Duration::hours(2);
        let window_id = seed_window(&pool, school_id, opens, closes, false, true, admin_id).await;

        for student_id in [s_a, s_b] {
            sqlx::query(
                "INSERT INTO checkin_submissions (id, window_id, student_id, submitted_at, method, is_late)
                 VALUES (gen_random_uuid(), $1, $2, NOW(), 'manual', FALSE)",
            )
            .bind(window_id)
            .bind(student_id)
            .execute(&pool)
            .await
            .unwrap();
        }

        let app = init_service(
            App::new()
                .app_data(web::Data::new(pool.clone()))
                .configure(crate::routes::configure_routes),
        )
        .await;
        let token = login_as(&app, &format!("fh_teacher_{}", suffix)).await;

        let req = TestRequest::get()
            .uri(&format!(
                "/api/v1/check-ins/windows/{}/submissions?homeroom_id={}",
                window_id, class_a
            ))
            .insert_header(("Authorization", format!("Bearer {}", token)))
            .to_request();
        let resp = call_service(&app, req).await;
        assert_eq!(resp.status(), 200);
        let body: Value = read_body_json(resp).await;
        let subs = body.as_array().unwrap();
        assert_eq!(subs.len(), 1, "only class_a student should appear");
        assert_eq!(
            subs[0]["student_id"].as_str().unwrap(),
            s_a.to_string()
        );
    }

    /// date_from filter excludes submissions before the given date.
    #[actix_web::test]
    #[ignore = "requires DATABASE_URL"]
    async fn test_filter_date_from_excludes_earlier_submissions() {
        let pool = test_pool().await;
        let suffix = Uuid::new_v4().to_string()[..8].to_string();

        let admin_id = seed_user(&pool, &format!("df_admin_{}", suffix), "Administrator").await;
        let teacher_id = seed_user(&pool, &format!("df_teacher_{}", suffix), "Teacher").await;
        let student_id = seed_user(&pool, &format!("df_student_{}", suffix), "Student").await;

        let school_id = seed_school(&pool, &format!("df_{}", suffix)).await;
        seed_school_assignment(&pool, teacher_id, school_id).await;
        seed_class_with_student(&pool, school_id, teacher_id, student_id, &format!("df_{}", suffix)).await;

        let opens = Utc::now() - Duration::days(5);
        let closes = Utc::now() + Duration::days(5);
        let window_id = seed_window(&pool, school_id, opens, closes, true, true, admin_id).await;

        // One submission 3 days ago, one today.
        let old_sub = Uuid::new_v4();
        let new_sub = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO checkin_submissions (id, window_id, student_id, submitted_at, method, is_late)
             VALUES ($1, $2, $3, NOW() - INTERVAL '3 days', 'manual', FALSE)",
        )
        .bind(old_sub)
        .bind(window_id)
        .bind(student_id)
        .execute(&pool)
        .await
        .unwrap();
        // Need a second student for the second submission (unique constraint).
        let student2 = seed_user(&pool, &format!("df_student2_{}", suffix), "Student").await;
        seed_class_with_student(&pool, school_id, teacher_id, student2, &format!("df2_{}", suffix)).await;
        sqlx::query(
            "INSERT INTO checkin_submissions (id, window_id, student_id, submitted_at, method, is_late)
             VALUES ($1, $2, $3, NOW(), 'manual', FALSE)",
        )
        .bind(new_sub)
        .bind(window_id)
        .bind(student2)
        .execute(&pool)
        .await
        .unwrap();

        let app = init_service(
            App::new()
                .app_data(web::Data::new(pool.clone()))
                .configure(crate::routes::configure_routes),
        )
        .await;
        let token = login_as(&app, &format!("df_teacher_{}", suffix)).await;

        // Filter to yesterday or later — old submission should be excluded.
        let yesterday = (Utc::now() - Duration::days(1))
            .format("%Y-%m-%d")
            .to_string();
        let req = TestRequest::get()
            .uri(&format!(
                "/api/v1/check-ins/windows/{}/submissions?date_from={}",
                window_id, yesterday
            ))
            .insert_header(("Authorization", format!("Bearer {}", token)))
            .to_request();
        let resp = call_service(&app, req).await;
        assert_eq!(resp.status(), 200);
        let body: Value = read_body_json(resp).await;
        let subs = body.as_array().unwrap();
        assert_eq!(subs.len(), 1, "only today's submission should appear");
        assert_eq!(subs[0]["submission_id"].as_str().unwrap(), new_sub.to_string());
    }

    /// homerooms endpoint returns classes for the window's school.
    #[actix_web::test]
    #[ignore = "requires DATABASE_URL"]
    async fn test_list_window_homerooms() {
        let pool = test_pool().await;
        let suffix = Uuid::new_v4().to_string()[..8].to_string();

        let admin_id = seed_user(&pool, &format!("hr_admin_{}", suffix), "Administrator").await;
        let teacher_id = seed_user(&pool, &format!("hr_teacher_{}", suffix), "Teacher").await;
        let student_id = seed_user(&pool, &format!("hr_student_{}", suffix), "Student").await;

        let school_id = seed_school(&pool, &format!("hr_{}", suffix)).await;
        seed_school_assignment(&pool, teacher_id, school_id).await;
        seed_class_with_student(&pool, school_id, teacher_id, student_id, &format!("hrC_{}", suffix)).await;

        let opens = Utc::now() - Duration::hours(1);
        let closes = Utc::now() + Duration::hours(1);
        let window_id = seed_window(&pool, school_id, opens, closes, false, true, admin_id).await;

        let app = init_service(
            App::new()
                .app_data(web::Data::new(pool.clone()))
                .configure(crate::routes::configure_routes),
        )
        .await;
        let token = login_as(&app, &format!("hr_teacher_{}", suffix)).await;

        let req = TestRequest::get()
            .uri(&format!("/api/v1/check-ins/windows/{}/homerooms", window_id))
            .insert_header(("Authorization", format!("Bearer {}", token)))
            .to_request();
        let resp = call_service(&app, req).await;
        assert_eq!(resp.status(), 200);
        let body: Value = read_body_json(resp).await;
        let hrs = body.as_array().unwrap();
        assert!(!hrs.is_empty(), "homerooms list must contain the seeded class");
        assert!(
            hrs.iter().any(|h| h["name"]
                .as_str()
                .map(|n| n.contains("hrC_"))
                .unwrap_or(false)),
            "seeded class must appear in homerooms"
        );
    }

    /// Invalid date format returns 422.
    #[actix_web::test]
    #[ignore = "requires DATABASE_URL"]
    async fn test_filter_invalid_date_format_rejected() {
        let pool = test_pool().await;
        let suffix = Uuid::new_v4().to_string()[..8].to_string();

        let admin_id = seed_user(&pool, &format!("inv_admin_{}", suffix), "Administrator").await;
        let teacher_id = seed_user(&pool, &format!("inv_teacher_{}", suffix), "Teacher").await;
        let school_id = seed_school(&pool, &format!("inv_{}", suffix)).await;
        seed_school_assignment(&pool, teacher_id, school_id).await;

        let opens = Utc::now() - Duration::hours(1);
        let closes = Utc::now() + Duration::hours(1);
        let window_id = seed_window(&pool, school_id, opens, closes, false, true, admin_id).await;

        let app = init_service(
            App::new()
                .app_data(web::Data::new(pool.clone()))
                .configure(crate::routes::configure_routes),
        )
        .await;
        let token = login_as(&app, &format!("inv_teacher_{}", suffix)).await;

        let req = TestRequest::get()
            .uri(&format!(
                "/api/v1/check-ins/windows/{}/submissions?date_from=03/01/2026",
                window_id
            ))
            .insert_header(("Authorization", format!("Bearer {}", token)))
            .to_request();
        let resp = call_service(&app, req).await;
        assert_eq!(resp.status(), 422, "MM/DD/YYYY should be rejected as invalid date");
    }

    /// school_id filter: student assigned to school_A's submissions are visible
    /// when filtering by school_A, and absent when filtering by school_B.
    #[actix_web::test]
    #[ignore = "requires DATABASE_URL"]
    async fn test_filter_by_school_id() {
        let pool = test_pool().await;
        let suffix = Uuid::new_v4().to_string()[..8].to_string();

        // Two independent schools.
        let school_a = seed_school(&pool, &format!("schA_{}", suffix)).await;
        let school_b = seed_school(&pool, &format!("schB_{}", suffix)).await;

        let admin_id =
            seed_user(&pool, &format!("sch_admin_{}", suffix), "Administrator").await;
        let teacher_id =
            seed_user(&pool, &format!("sch_teacher_{}", suffix), "Teacher").await;
        let student_id =
            seed_user(&pool, &format!("sch_student_{}", suffix), "Student").await;

        // Teacher is assigned to school_a; student is enrolled there.
        seed_school_assignment(&pool, teacher_id, school_a).await;
        seed_school_assignment(&pool, student_id, school_a).await;
        seed_class_with_student(
            &pool,
            school_a,
            teacher_id,
            student_id,
            &format!("sch_{}", suffix),
        )
        .await;

        // Window belongs to school_a.
        let opens = Utc::now() - Duration::hours(1);
        let closes = Utc::now() + Duration::hours(1);
        let window_id =
            seed_window(&pool, school_a, opens, closes, false, true, admin_id).await;

        // Submit a check-in as the student.
        sqlx::query(
            "INSERT INTO checkin_submissions
                 (id, window_id, student_id, submitted_at, method)
             VALUES (gen_random_uuid(), $1, $2, NOW(), 'manual')",
        )
        .bind(window_id)
        .bind(student_id)
        .execute(&pool)
        .await
        .expect("seed submission failed");

        let app = init_service(
            App::new()
                .app_data(web::Data::new(pool.clone()))
                .configure(crate::routes::configure_routes),
        )
        .await;
        let token = login_as(&app, &format!("sch_admin_{}", suffix)).await;

        // Filter by school_a → should return the submission.
        let resp = call_service(
            &app,
            TestRequest::get()
                .uri(&format!(
                    "/api/v1/check-ins/windows/{}/submissions?school_id={}",
                    window_id, school_a
                ))
                .insert_header(("Authorization", format!("Bearer {}", token)))
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), 200);
        let body: Value = read_body_json(resp).await;
        assert_eq!(
            body.as_array().unwrap().len(),
            1,
            "school_a filter should return the one submission"
        );

        // Filter by school_b → submission must not appear.
        let resp = call_service(
            &app,
            TestRequest::get()
                .uri(&format!(
                    "/api/v1/check-ins/windows/{}/submissions?school_id={}",
                    window_id, school_b
                ))
                .insert_header(("Authorization", format!("Bearer {}", token)))
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), 200);
        let body: Value = read_body_json(resp).await;
        assert_eq!(
            body.as_array().unwrap().len(),
            0,
            "school_b filter should return no submissions"
        );
    }
}
