/// Check-in dashboard for Students and Parents.
///
/// Students see the currently relevant window (open → accepting_late →
/// upcoming → most-recent) and can submit with one tap.
///
/// Parents see each linked student alongside their check-in status for the
/// same window and can submit on their behalf.
use yew::prelude::*;
use yew_router::prelude::*;
use wasm_bindgen_futures::spawn_local;

use crate::api::checkins::{self, CheckinWindow, LinkedStudent, MyCheckin};
use crate::api::notifications::{self, Notification};
use crate::router::Route;
use crate::state::AppStateContext;

// ---------------------------------------------------------------------------
// Page-level state
// ---------------------------------------------------------------------------

#[derive(Clone, PartialEq)]
enum PageState {
    Loading,
    Ready,
    Error(String),
}

// ---------------------------------------------------------------------------
// Component
// ---------------------------------------------------------------------------

#[function_component(CheckinPage)]
pub fn checkin_page() -> Html {
    let state = use_context::<AppStateContext>().expect("AppState context not found");
    let navigator = use_navigator().unwrap();

    // Auth + role guard
    let is_auth = state.is_authenticated();
    let has_role = state.has_role("Student") || state.has_role("Parent");
    let is_parent = state.has_role("Parent");

    {
        let navigator = navigator.clone();
        use_effect_with((is_auth, has_role), move |(auth, role_ok)| {
            if !auth {
                navigator.push(&Route::Login);
            } else if !role_ok {
                navigator.push(&Route::Unauthorized);
            }
        });
    }

    // Page state
    let page_state = use_state(|| PageState::Loading);
    let windows = use_state(Vec::<CheckinWindow>::new);
    let my_checkins = use_state(Vec::<MyCheckin>::new);
    let linked_students = use_state(Vec::<LinkedStudent>::new);
    let reminders = use_state(Vec::<Notification>::new);
    let submitting = use_state(|| false);
    // (success, message)
    let submit_result = use_state(|| Option::<(bool, String)>::None);

    // Load data on mount
    {
        let token = state.token.clone();
        let page_state = page_state.clone();
        let windows = windows.clone();
        let my_checkins = my_checkins.clone();
        let linked_students = linked_students.clone();
        let reminders = reminders.clone();
        let is_parent_flag = is_parent;

        use_effect_with((), move |_| {
            let Some(token) = token else { return; };

            let page_state2 = page_state.clone();
            let windows2 = windows.clone();
            let my_checkins2 = my_checkins.clone();
            let linked_students2 = linked_students.clone();
            let reminders2 = reminders.clone();

            spawn_local(async move {
                // Fetch windows
                match checkins::list_windows(&token).await {
                    Ok(w) => windows2.set(w),
                    Err(e) => {
                        page_state2.set(PageState::Error(format!("Failed to load windows: {}", e)));
                        return;
                    }
                }

                // Fetch my check-in history
                match checkins::my_checkins(&token).await {
                    Ok(c) => my_checkins2.set(c),
                    Err(_) => {} // non-fatal; history may be empty
                }

                // Fetch linked students (parents only)
                if is_parent_flag {
                    match checkins::linked_students(&token).await {
                        Ok(s) => linked_students2.set(s),
                        Err(_) => {}
                    }
                }

                let _ = notifications::generate_reminders(&token).await;
                if let Ok(items) = notifications::list_notifications(&token).await {
                    let reminder_items: Vec<Notification> = items
                        .into_iter()
                        .filter(|n| {
                            n.subject.starts_with("Upcoming check-in")
                                || n.subject.starts_with("Missed check-in")
                        })
                        .take(3)
                        .collect();
                    reminders2.set(reminder_items);
                }

                page_state2.set(PageState::Ready);
            });
        });
    }

    // Identify the best window to act on: open > accepting_late > upcoming > any
    let active_window: Option<CheckinWindow> = {
        let ws = (*windows).clone();
        ws.iter()
            .find(|w| w.status == "open")
            .or_else(|| ws.iter().find(|w| w.status == "accepting_late"))
            .or_else(|| ws.iter().find(|w| w.status == "upcoming"))
            .or_else(|| ws.first())
            .cloned()
    };

    // Build a callback that submits on behalf of a student
    let make_submit = {
        let token = state.token.clone();
        let active_window = active_window.clone();
        let submitting = submitting.clone();
        let submit_result = submit_result.clone();
        let my_checkins = my_checkins.clone();

        move |student_id_opt: Option<String>| {
            let token = token.clone();
            let active_window = active_window.clone();
            let submitting = submitting.clone();
            let submit_result = submit_result.clone();
            let my_checkins = my_checkins.clone();
            let sid_opt = student_id_opt.clone();

            Callback::from(move |_: MouseEvent| {
                let Some(ref window) = active_window else { return; };
                let Some(ref tok) = token else { return; };

                if *submitting { return; }

                submitting.set(true);
                submit_result.set(None);

                let window_id = window.id.clone();
                let tok = tok.clone();
                let submitting2 = submitting.clone();
                let submit_result2 = submit_result.clone();
                let my_checkins2 = my_checkins.clone();
                let sid = sid_opt.clone();

                spawn_local(async move {
                    let result = checkins::submit_checkin(
                        &window_id,
                        None,
                        sid.as_deref(),
                        &tok,
                    )
                    .await;

                    match result {
                        Ok(resp) => {
                            let msg = if resp.is_late {
                                "Late check-in submitted — pending review.".to_string()
                            } else {
                                "Check-in submitted — pending review.".to_string()
                            };
                            submit_result2.set(Some((true, msg)));
                            // Refresh history
                            if let Ok(updated) = checkins::my_checkins(&tok).await {
                                my_checkins2.set(updated);
                            }
                        }
                        Err(e) => {
                            let msg = match e {
                                crate::api::client::ApiError::Http { status: 409, .. } => {
                                    "You have already checked in for this window.".to_string()
                                }
                                crate::api::client::ApiError::Http { status: 422, message } => {
                                    // Extract server error message
                                    serde_json::from_str::<serde_json::Value>(&message)
                                        .ok()
                                        .and_then(|v| v["error"].as_str().map(|s| s.to_string()))
                                        .unwrap_or(message)
                                }
                                _ => "Check-in failed. Please try again.".to_string(),
                            };
                            submit_result2.set(Some((false, msg)));
                        }
                    }
                    submitting2.set(false);
                });
            })
        }
    };

    if !is_auth || !has_role {
        return html! { <div class="loading">{ "Checking access\u{2026}" }</div> };
    }

    match *page_state {
        PageState::Loading => html! { <div class="loading">{ "Loading check-in\u{2026}" }</div> },
        PageState::Error(ref msg) => html! {
            <div class="card">
                <p class="error-msg">{ msg.clone() }</p>
            </div>
        },
        PageState::Ready => html! {
            <div>
                <div class="card">
                    <h2>{ "Daily Check-In" }</h2>
                </div>

                // Submit result banner
                if let Some((success, ref msg)) = *submit_result {
                    <div class={if success { "banner banner-success" } else { "banner banner-error" }}>
                        { msg.clone() }
                    </div>
                }

                if is_parent {
                    { render_parent_view(
                        &active_window,
                        &my_checkins,
                        &linked_students,
                        &make_submit,
                        *submitting,
                    ) }
                } else {
                    { render_student_view(
                        &active_window,
                        &my_checkins,
                        &make_submit,
                        *submitting,
                    ) }
                }

                if !(*reminders).is_empty() {
                    <div class="card">
                        <h3>{ "Reminders" }</h3>
                        <div class="notification-list compact">
                            { for (*reminders).iter().map(|n| html! {
                                <div key={n.id.clone()} class="notification-item reminder">
                                    <div class="notif-header">
                                        <span class="notif-type checkin">{ "Reminder" }</span>
                                        <span class="notif-time">{ fmt_time(&n.created_at) }</span>
                                    </div>
                                    <div class="notif-subject">{ &n.subject }</div>
                                    <div class="notif-body">{ &n.body }</div>
                                </div>
                            }) }
                        </div>
                    </div>
                }

                // Recent check-in history
                if !(*my_checkins).is_empty() {
                    <div class="card">
                        <h3>{ "Recent Check-Ins" }</h3>
                        <table class="data-table">
                            <thead>
                                <tr>
                                    <th>{ "Window" }</th>
                                    <th>{ if is_parent { "Student" } else { "Submitted" } }</th>
                                    <th>{ "Status" }</th>
                                    <th>{ "Decision" }</th>
                                </tr>
                            </thead>
                            <tbody>
                                { for (*my_checkins).iter().map(|c| html! {
                                    <tr key={c.submission_id.clone()}>
                                        <td>{ &c.window_title }</td>
                                        <td>{ if is_parent { c.student_username.clone() } else { c.submitted_at[..10].to_string() } }</td>
                                        <td>{ if c.is_late { "Late" } else { "On time" } }</td>
                                        <td>
                                            <span class={decision_css(&c.decision)}>
                                                { decision_label(&c.decision) }
                                            </span>
                                            if c.decision == "rejected" {
                                                if let Some(ref r) = c.reason {
                                                    <span class="denial-reason">{ format!(" — {}", r) }</span>
                                                }
                                            }
                                        </td>
                                    </tr>
                                }) }
                            </tbody>
                        </table>
                    </div>
                }
            </div>
        },
    }
}

// ---------------------------------------------------------------------------
// Sub-renderers
// ---------------------------------------------------------------------------

fn render_student_view(
    active_window: &Option<CheckinWindow>,
    my_checkins: &UseStateHandle<Vec<MyCheckin>>,
    make_submit: &impl Fn(Option<String>) -> Callback<MouseEvent>,
    submitting: bool,
) -> Html {
    let Some(window) = active_window else {
        return html! {
            <div class="card">
                <p>{ "No check-in windows are available for your school right now." }</p>
            </div>
        };
    };

    let existing: Option<&MyCheckin> = (**my_checkins)
        .iter()
        .find(|c| c.window_id == window.id);

    html! {
        <div class="card">
            <h3>{ &window.title }</h3>
            <p class="school-name">{ &window.school_name }</p>
            { render_window_status_badge(&window.status) }
            <p class="window-times">
                { format!("Opens: {} · Closes: {}", fmt_time(&window.opens_at), fmt_time(&window.closes_at)) }
                if window.allow_late {
                    <span class="late-tag">{ " (late accepted)" }</span>
                }
            </p>

            if let Some(existing_checkin) = existing {
                <div class="submission-status">
                    <p>{ "You have already submitted." }</p>
                    <span class={decision_css(&existing_checkin.decision)}>
                        { decision_label(&existing_checkin.decision) }
                    </span>
                    if existing_checkin.decision == "rejected" {
                        if let Some(ref r) = existing_checkin.reason {
                            <p class="denial-reason">{ format!("Reason: {}", r) }</p>
                        }
                    }
                </div>
            } else if window.status == "open" || window.status == "accepting_late" {
                <button
                    class="btn-primary"
                    onclick={make_submit(None)}
                    disabled={submitting}
                >
                    { if submitting { "Submitting\u{2026}" } else { "Check In Now" } }
                </button>
            } else if window.status == "upcoming" {
                <p class="reminder">{ format!("Opens at {}", fmt_time(&window.opens_at)) }</p>
            } else {
                <p class="closed-msg">{ "This check-in window is closed." }</p>
            }
        </div>
    }
}

fn render_parent_view(
    active_window: &Option<CheckinWindow>,
    my_checkins: &UseStateHandle<Vec<MyCheckin>>,
    linked_students: &UseStateHandle<Vec<LinkedStudent>>,
    make_submit: &impl Fn(Option<String>) -> Callback<MouseEvent>,
    submitting: bool,
) -> Html {
    let Some(window) = active_window else {
        return html! {
            <div class="card">
                <p>{ "No check-in windows are available right now." }</p>
            </div>
        };
    };

    html! {
        <div>
            <div class="card">
                <h3>{ &window.title }</h3>
                <p class="school-name">{ &window.school_name }</p>
                { render_window_status_badge(&window.status) }
                <p class="window-times">
                    { format!("Opens: {} · Closes: {}", fmt_time(&window.opens_at), fmt_time(&window.closes_at)) }
                    if window.allow_late {
                        <span class="late-tag">{ " (late accepted)" }</span>
                    }
                </p>
            </div>

            if (*linked_students).is_empty() {
                <div class="card">
                    <p>{ "No linked students found. Contact your school administrator." }</p>
                </div>
            }

            { for (*linked_students).iter().map(|student| {
                let display = student.display_name.as_deref()
                    .unwrap_or(&student.username)
                    .to_string();

                let existing: Option<&MyCheckin> = (**my_checkins)
                    .iter()
                    .find(|c| c.window_id == window.id && c.student_username == student.username);

                let sid = student.id.clone();

                html! {
                    <div class="card student-checkin-card" key={student.id.clone()}>
                        <h4>{ display }</h4>

                        if let Some(ec) = existing {
                            <div class="submission-status">
                                <p>{ "Already checked in." }</p>
                                <span class={decision_css(&ec.decision)}>
                                    { decision_label(&ec.decision) }
                                </span>
                                if ec.decision == "rejected" {
                                    if let Some(ref r) = ec.reason {
                                        <p class="denial-reason">{ format!("Reason: {}", r) }</p>
                                    }
                                }
                            </div>
                        } else if window.status == "open" || window.status == "accepting_late" {
                            <button
                                class="btn-primary"
                                onclick={make_submit(Some(sid))}
                                disabled={submitting}
                            >
                                { if submitting { "Submitting\u{2026}" } else { "Check In" } }
                            </button>
                        } else if window.status == "upcoming" {
                            <p class="reminder">{ format!("Window opens at {}", fmt_time(&window.opens_at)) }</p>
                        } else {
                            <p class="closed-msg">{ "Window closed." }</p>
                        }
                    </div>
                }
            }) }
        </div>
    }
}

// ---------------------------------------------------------------------------
// Formatting helpers
// ---------------------------------------------------------------------------

fn render_window_status_badge(status: &str) -> Html {
    let (label, css) = match status {
        "open" => ("Open", "badge badge-open"),
        "accepting_late" => ("Accepting Late", "badge badge-late"),
        "upcoming" => ("Upcoming", "badge badge-upcoming"),
        _ => ("Closed", "badge badge-closed"),
    };
    html! { <span class={css}>{ label }</span> }
}

fn decision_css(decision: &str) -> &'static str {
    match decision {
        "approved" => "decision-badge approved",
        "rejected" => "decision-badge rejected",
        _ => "decision-badge pending",
    }
}

fn decision_label(decision: &str) -> &'static str {
    match decision {
        "approved" => "Approved",
        "rejected" => "Denied",
        _ => "Pending",
    }
}

/// Format an ISO-8601 timestamp down to a human-readable date+time.
/// Falls back to the raw string if parsing fails.
fn fmt_time(iso: &str) -> String {
    // Take up to "YYYY-MM-DDTHH:MM" (16 chars) and replace 'T' with a space.
    iso.get(..16)
        .map(|s| s.replace('T', " "))
        .unwrap_or_else(|| iso.to_string())
}
