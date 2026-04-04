use serde::{Deserialize, Serialize};

use super::client::{get, post, ApiError};

// ---------------------------------------------------------------------------
// Response types (mirrors backend JSON shapes)
// ---------------------------------------------------------------------------

#[derive(Deserialize, Clone, PartialEq)]
pub struct CheckinWindow {
    pub id: String,
    pub title: String,
    pub description: Option<String>,
    pub opens_at: String,
    pub closes_at: String,
    pub allow_late: bool,
    pub active: bool,
    pub school_id: String,
    pub school_name: String,
    /// "upcoming" | "open" | "accepting_late" | "closed"
    pub status: String,
}

#[derive(Deserialize, Clone, PartialEq)]
pub struct MyCheckin {
    pub submission_id: String,
    pub window_id: String,
    pub window_title: String,
    pub opens_at: String,
    pub closes_at: String,
    pub submitted_at: String,
    pub is_late: bool,
    pub method: String,
    pub decision: String,
    pub reason: Option<String>,
    pub decided_at: Option<String>,
    pub student_username: String,
}

#[derive(Deserialize, Clone, PartialEq)]
pub struct SubmissionRecord {
    pub submission_id: String,
    pub window_id: String,
    pub window_title: String,
    pub student_id: String,
    pub username: String,
    pub display_name: Option<String>,
    pub submitted_at: String,
    pub is_late: bool,
    pub method: String,
    pub notes: Option<String>,
    pub decision: String,
    pub reason: Option<String>,
    pub decided_at: Option<String>,
    pub decided_by_name: Option<String>,
}

#[derive(Deserialize, Clone, PartialEq)]
pub struct SubmitResponse {
    pub submission_id: String,
    pub window_id: String,
    pub window_title: String,
    pub student_id: String,
    pub submitted_at: String,
    pub is_late: bool,
    pub status: String,
}

#[derive(Deserialize, Clone, PartialEq)]
pub struct LinkedStudent {
    pub id: String,
    pub username: String,
    pub display_name: Option<String>,
}

#[derive(Deserialize, Clone, PartialEq)]
pub struct HomeroomOption {
    pub id: String,
    pub name: String,
    pub grade_level: Option<String>,
}

/// Optional server-side filters for `list_submissions`.
#[derive(Default, Clone, PartialEq)]
pub struct SubmissionFilters {
    /// UUID string of a school, or empty string for no filter.
    pub school_id: String,
    /// `"all"` or one of `"pending"` | `"approved"` | `"rejected"`.
    pub decision: String,
    /// UUID string of a homeroom/class, or empty string for no filter.
    pub homeroom_id: String,
    /// Date string `YYYY-MM-DD`, or empty for no lower bound.
    pub date_from: String,
    /// Date string `YYYY-MM-DD`, or empty for no upper bound.
    pub date_to: String,
}

// ---------------------------------------------------------------------------
// Request body types
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct SubmitBody<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    notes: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    student_id: Option<&'a str>,
}

#[derive(Serialize)]
struct DecideBody<'a> {
    decision: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<&'a str>,
}

// ---------------------------------------------------------------------------
// API functions
// ---------------------------------------------------------------------------

/// Fetch all check-in windows visible to the current user.
pub async fn list_windows(token: &str) -> Result<Vec<CheckinWindow>, ApiError> {
    get::<Vec<CheckinWindow>>("/check-ins/windows", Some(token)).await
}

/// Fetch this user's own check-in history (Student) or linked students'
/// history (Parent).
pub async fn my_checkins(token: &str) -> Result<Vec<MyCheckin>, ApiError> {
    get::<Vec<MyCheckin>>("/check-ins/my", Some(token)).await
}

/// Submit a check-in for the given window.
/// `student_id` must be provided when the caller is a Parent.
pub async fn submit_checkin(
    window_id: &str,
    notes: Option<&str>,
    student_id: Option<&str>,
    token: &str,
) -> Result<SubmitResponse, ApiError> {
    let body = SubmitBody { notes, student_id };
    post::<_, SubmitResponse>(
        &format!("/check-ins/windows/{}/submit", window_id),
        &body,
        Some(token),
    )
    .await
}

/// Fetch submissions for a specific window, with optional server-side filters.
pub async fn list_submissions(
    window_id: &str,
    filters: &SubmissionFilters,
    token: &str,
) -> Result<Vec<SubmissionRecord>, ApiError> {
    let mut params: Vec<String> = Vec::new();
    if !filters.school_id.is_empty() {
        params.push(format!("school_id={}", filters.school_id));
    }
    if filters.decision != "all" && !filters.decision.is_empty() {
        params.push(format!("decision={}", filters.decision));
    }
    if !filters.homeroom_id.is_empty() {
        params.push(format!("homeroom_id={}", filters.homeroom_id));
    }
    if !filters.date_from.is_empty() {
        params.push(format!("date_from={}", filters.date_from));
    }
    if !filters.date_to.is_empty() {
        params.push(format!("date_to={}", filters.date_to));
    }
    let path = if params.is_empty() {
        format!("/check-ins/windows/{}/submissions", window_id)
    } else {
        format!(
            "/check-ins/windows/{}/submissions?{}",
            window_id,
            params.join("&")
        )
    };
    get::<Vec<SubmissionRecord>>(&path, Some(token)).await
}

/// Fetch the active homerooms (classes) for the school owning a window.
pub async fn list_homerooms(
    window_id: &str,
    token: &str,
) -> Result<Vec<HomeroomOption>, ApiError> {
    get::<Vec<HomeroomOption>>(
        &format!("/check-ins/windows/{}/homerooms", window_id),
        Some(token),
    )
    .await
}

/// Approve or reject a submission.
pub async fn decide_submission(
    window_id: &str,
    submission_id: &str,
    decision: &str,
    reason: Option<&str>,
    token: &str,
) -> Result<serde_json::Value, ApiError> {
    let body = DecideBody { decision, reason };
    post::<_, serde_json::Value>(
        &format!(
            "/check-ins/windows/{}/submissions/{}/decide",
            window_id, submission_id
        ),
        &body,
        Some(token),
    )
    .await
}

/// Returns the list of students linked to the calling Parent.
pub async fn linked_students(token: &str) -> Result<Vec<LinkedStudent>, ApiError> {
    get::<Vec<LinkedStudent>>("/users/me/linked-students", Some(token)).await
}
