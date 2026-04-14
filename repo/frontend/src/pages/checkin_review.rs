/// Check-in review queue for Teachers and Academic Staff.
///
/// Shows the list of check-in windows for the reviewer's schools.
/// Clicking a window loads its submissions. Submissions can be approved
/// or denied (denial requires a non-empty reason).
use yew::prelude::*;
use yew_router::prelude::*;
use wasm_bindgen_futures::spawn_local;

use crate::api::checkins::{self, CheckinWindow, HomeroomOption, SubmissionFilters, SubmissionRecord};
use crate::router::Route;
use crate::state::AppStateContext;

// ---------------------------------------------------------------------------
// Component
// ---------------------------------------------------------------------------

#[function_component(CheckinReviewPage)]
pub fn checkin_review_page() -> Html {
    let state = use_context::<AppStateContext>().expect("AppState context not found");
    let navigator = use_navigator().unwrap();

    let is_auth = state.is_authenticated();
    let has_role =
        state.has_role("Teacher") || state.has_role("AcademicStaff") || state.has_role("Administrator");

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

    // State
    let windows = use_state(Vec::<CheckinWindow>::new);
    let selected_window_id = use_state(|| Option::<String>::None);
    let submissions = use_state(Vec::<SubmissionRecord>::new);
    let homerooms = use_state(Vec::<HomeroomOption>::new);
    let filter_school_id = use_state(|| String::new());
    let filter_decision = use_state(|| "pending".to_string()); // "all" | "pending" | "approved" | "rejected"
    let filter_homeroom = use_state(|| String::new());
    let filter_date_from = use_state(|| String::new());
    let filter_date_to = use_state(|| String::new());
    let loading_submissions = use_state(|| false);
    let windows_loaded = use_state(|| false);

    // Decide modal state
    let modal_open = use_state(|| false);
    let modal_submission_id = use_state(|| String::new());
    let modal_window_id = use_state(|| String::new());
    let modal_decision = use_state(|| "approved".to_string());
    let modal_reason = use_state(|| String::new());
    let deciding = use_state(|| false);
    let decide_msg = use_state(|| Option::<(bool, String)>::None);

    // Load windows on mount
    {
        let token = state.token.clone();
        let windows = windows.clone();
        let windows_loaded = windows_loaded.clone();

        use_effect_with((), move |_| {
            let Some(token) = token else { return; };
            let windows2 = windows.clone();
            let wl = windows_loaded.clone();
            spawn_local(async move {
                if let Ok(ws) = checkins::list_windows(&token).await {
                    windows2.set(ws);
                }
                wl.set(true);
            });
        });
    }

    // Load homerooms when a window is selected
    {
        let token = state.token.clone();
        let homerooms = homerooms.clone();

        use_effect_with((*selected_window_id).clone(), move |wid| {
            let Some(window_id) = wid.clone() else {
                homerooms.set(vec![]);
                return;
            };
            let Some(token) = token else { return; };
            let homerooms2 = homerooms.clone();
            spawn_local(async move {
                if let Ok(hrs) = checkins::list_homerooms(&window_id, &token).await {
                    homerooms2.set(hrs);
                } else {
                    homerooms2.set(vec![]);
                }
            });
        });
    }

    // Load submissions when window or any filter changes
    {
        let token = state.token.clone();
        let selected = selected_window_id.clone();
        let submissions = submissions.clone();
        let loading = loading_submissions.clone();
        let fs = filter_school_id.clone();
        let fd = filter_decision.clone();
        let fh = filter_homeroom.clone();
        let ff = filter_date_from.clone();
        let ft = filter_date_to.clone();

        use_effect_with(
            (
                (*selected_window_id).clone(),
                (*filter_school_id).clone(),
                (*filter_decision).clone(),
                (*filter_homeroom).clone(),
                (*filter_date_from).clone(),
                (*filter_date_to).clone(),
            ),
            move |(wid, _, _, _, _, _)| {
                let Some(window_id) = wid.clone() else { return; };
                let Some(token) = token else { return; };
                let submissions2 = submissions.clone();
                let loading2 = loading.clone();
                let filters = SubmissionFilters {
                    school_id: (*fs).clone(),
                    decision: (*fd).clone(),
                    homeroom_id: (*fh).clone(),
                    date_from: (*ff).clone(),
                    date_to: (*ft).clone(),
                };
                loading2.set(true);
                spawn_local(async move {
                    match checkins::list_submissions(&window_id, &filters, &token).await {
                        Ok(subs) => submissions2.set(subs),
                        Err(_) => submissions2.set(vec![]),
                    }
                    loading2.set(false);
                });
                let _ = selected;
            },
        );
    }

    // All filtering is now server-side; `submissions` is already the filtered list.
    let filtered: Vec<SubmissionRecord> = (*submissions).clone();

    // Open modal for a submission
    let open_modal = {
        let modal_open = modal_open.clone();
        let modal_submission_id = modal_submission_id.clone();
        let modal_window_id = modal_window_id.clone();
        let modal_decision = modal_decision.clone();
        let modal_reason = modal_reason.clone();
        let decide_msg = decide_msg.clone();
        move |sub: SubmissionRecord| {
            modal_submission_id.set(sub.submission_id.clone());
            modal_window_id.set(sub.window_id.clone());
            modal_decision.set("approved".to_string());
            modal_reason.set(String::new());
            decide_msg.set(None);
            modal_open.set(true);
        }
    };

    // Submit decision from modal
    let on_decide = {
        let token = state.token.clone();
        let modal_open = modal_open.clone();
        let modal_submission_id = modal_submission_id.clone();
        let modal_window_id = modal_window_id.clone();
        let modal_decision = modal_decision.clone();
        let modal_reason = modal_reason.clone();
        let deciding = deciding.clone();
        let decide_msg = decide_msg.clone();
        let submissions = submissions.clone();
        let fs2 = filter_school_id.clone();
        let fd2 = filter_decision.clone();
        let fh2 = filter_homeroom.clone();
        let ff2 = filter_date_from.clone();
        let ft2 = filter_date_to.clone();

        Callback::from(move |_: MouseEvent| {
            let Some(ref token) = token else { return; };
            if *deciding { return; }

            let decision = (*modal_decision).clone();
            let reason = (*modal_reason).clone();
            let submission_id = (*modal_submission_id).clone();
            let window_id = (*modal_window_id).clone();
            let token = token.clone();
            let deciding2 = deciding.clone();
            let decide_msg2 = decide_msg.clone();
            let modal_open2 = modal_open.clone();
            let submissions2 = submissions.clone();
            // Clone filter handles so they remain available on subsequent clicks.
            let fs3 = fs2.clone();
            let fd3 = fd2.clone();
            let fh3 = fh2.clone();
            let ff3 = ff2.clone();
            let ft3 = ft2.clone();

            deciding2.set(true);

            spawn_local(async move {
                let reason_opt = if reason.trim().is_empty() {
                    None
                } else {
                    Some(reason.as_str())
                };

                let result = checkins::decide_submission(
                    &window_id,
                    &submission_id,
                    &decision,
                    reason_opt,
                    &token,
                )
                .await;

                match result {
                    Ok(_) => {
                        // Refresh submissions with current filters
                        let filters = SubmissionFilters {
                            school_id: (*fs3).clone(),
                            decision: (*fd3).clone(),
                            homeroom_id: (*fh3).clone(),
                            date_from: (*ff3).clone(),
                            date_to: (*ft3).clone(),
                        };
                        if let Ok(subs) = checkins::list_submissions(&window_id, &filters, &token).await {
                            submissions2.set(subs);
                        }
                        modal_open2.set(false);
                    }
                    Err(e) => {
                        let msg = match e {
                            crate::api::client::ApiError::Http { message, .. } => {
                                serde_json::from_str::<serde_json::Value>(&message)
                                    .ok()
                                    .and_then(|v| v["error"].as_str().map(|s| s.to_string()))
                                    .unwrap_or(message)
                            }
                            other => other.to_string(),
                        };
                        decide_msg2.set(Some((false, msg)));
                    }
                }
                deciding2.set(false);
            });
        })
    };

    if !is_auth || !has_role {
        return html! { <div class="loading">{ "Checking access\u{2026}" }</div> };
    }

    // Compute unique schools from loaded windows before entering html! macro,
    // because Yew 0.21's html! parser does not support `let` bindings with
    // semicolons inside {} child expression blocks.
    let multi_school_select: Html = {
        let mut seen = std::collections::HashSet::new();
        let schools: Vec<(String, String)> = (*windows)
            .iter()
            .filter(|w| seen.insert(w.school_id.clone()))
            .map(|w| (w.school_id.clone(), w.school_name.clone()))
            .collect();
        if schools.len() > 1 {
            let fs = filter_school_id.clone();
            html! {
                <div class="filter-group">
                    <label class="filter-label" for="school-select">{ "School:" }</label>
                    <select
                        id="school-select"
                        value={(*filter_school_id).clone()}
                        onchange={Callback::from(move |e: Event| {
                            let sel: web_sys::HtmlSelectElement = e.target_unchecked_into();
                            fs.set(sel.value());
                        })}
                    >
                        <option value="">{ "All schools" }</option>
                        { for schools.into_iter().map(|(sid, sname)| html! {
                            <option key={sid.clone()} value={sid}>{ sname }</option>
                        }) }
                    </select>
                </div>
            }
        } else {
            html! {}
        }
    };

    html! {
        <div>
            <div class="card">
                <h2>{ "Check-In Review" }</h2>
                <p>{ "Select a window to review submissions." }</p>
            </div>

            <div class="review-layout">
                // Windows panel
                <div class="windows-panel card">
                    <h3>{ "Windows" }</h3>
                    if !*windows_loaded {
                        <p>{ "Loading\u{2026}" }</p>
                    } else if (*windows).is_empty() {
                        <p>{ "No windows available for your schools." }</p>
                    } else {
                        { for (*windows).iter().map(|w| {
                            let wid = w.id.clone();
                            let selected = (*selected_window_id).as_deref() == Some(&wid);
                            let on_click = {
                                let sel = selected_window_id.clone();
                                let wid2 = wid.clone();
                                Callback::from(move |_: MouseEvent| sel.set(Some(wid2.clone())))
                            };
                            html! {
                                <div
                                    key={w.id.clone()}
                                    class={if selected { "window-item selected" } else { "window-item" }}
                                    onclick={on_click}
                                >
                                    <div class="window-item-title">{ &w.title }</div>
                                    <div class="window-item-school">{ &w.school_name }</div>
                                    <span class={status_css(&w.status)}>{ &w.status }</span>
                                </div>
                            }
                        }) }
                    }
                </div>

                // Submissions panel
                <div class="submissions-panel">
                    if selected_window_id.is_none() {
                        <div class="card">
                            <p>{ "Select a window on the left to see submissions." }</p>
                        </div>
                    } else {
                        <div class="card">
                            // Filter bar
                            <div class="filter-bar">
                                // School filter (only shown when windows span > 1 school)
                                { multi_school_select.clone() }

                                // Decision filter buttons
                                <div class="filter-group">
                                    <span class="filter-label">{ "Status:" }</span>
                                    { for ["all", "pending", "approved", "rejected"].iter().map(|f| {
                                        let f_str = f.to_string();
                                        let current = (*filter_decision).clone();
                                        let fh = filter_decision.clone();
                                        html! {
                                            <button
                                                key={*f}
                                                class={if current == *f { "filter-btn active" } else { "filter-btn" }}
                                                onclick={Callback::from(move |_: MouseEvent| fh.set(f_str.clone()))}
                                            >
                                                { capitalize(f) }
                                            </button>
                                        }
                                    }) }
                                </div>

                                // Homeroom filter
                                if !(*homerooms).is_empty() {
                                    <div class="filter-group">
                                        <label class="filter-label" for="homeroom-select">{ "Homeroom:" }</label>
                                        <select
                                            id="homeroom-select"
                                            value={(*filter_homeroom).clone()}
                                            onchange={{
                                                let fh = filter_homeroom.clone();
                                                Callback::from(move |e: Event| {
                                                    let sel: web_sys::HtmlSelectElement = e.target_unchecked_into();
                                                    fh.set(sel.value());
                                                })
                                            }}
                                        >
                                            <option value="">{ "All homerooms" }</option>
                                            { for (*homerooms).iter().map(|hr| {
                                                let label = match &hr.grade_level {
                                                    Some(g) => format!("{} ({})", hr.name, g),
                                                    None => hr.name.clone(),
                                                };
                                                html! {
                                                    <option key={hr.id.clone()} value={hr.id.clone()}>
                                                        { label }
                                                    </option>
                                                }
                                            }) }
                                        </select>
                                    </div>
                                }

                                // Date range filter
                                <div class="filter-group">
                                    <label class="filter-label" for="date-from">{ "From:" }</label>
                                    <input
                                        id="date-from"
                                        type="date"
                                        value={(*filter_date_from).clone()}
                                        oninput={{
                                            let ff = filter_date_from.clone();
                                            Callback::from(move |e: InputEvent| {
                                                let inp: web_sys::HtmlInputElement = e.target_unchecked_into();
                                                ff.set(inp.value());
                                            })
                                        }}
                                    />
                                    <label class="filter-label" for="date-to">{ "To:" }</label>
                                    <input
                                        id="date-to"
                                        type="date"
                                        value={(*filter_date_to).clone()}
                                        oninput={{
                                            let ft = filter_date_to.clone();
                                            Callback::from(move |e: InputEvent| {
                                                let inp: web_sys::HtmlInputElement = e.target_unchecked_into();
                                                ft.set(inp.value());
                                            })
                                        }}
                                    />
                                </div>
                            </div>

                            if *loading_submissions {
                                <p>{ "Loading submissions\u{2026}" }</p>
                            } else if filtered.is_empty() {
                                <p>{ "No submissions match this filter." }</p>
                            } else {
                                <table class="data-table">
                                    <thead>
                                        <tr>
                                            <th>{ "Student" }</th>
                                            <th>{ "Submitted" }</th>
                                            <th>{ "Timing" }</th>
                                            <th>{ "Method" }</th>
                                            <th>{ "Status" }</th>
                                            <th>{ "Action" }</th>
                                        </tr>
                                    </thead>
                                    <tbody>
                                        { for filtered.iter().map(|sub| {
                                            let sub2 = sub.clone();
                                            let on_review = {
                                                let open = open_modal.clone();
                                                let s = sub.clone();
                                                Callback::from(move |_: MouseEvent| open(s.clone()))
                                            };
                                            html! {
                                                <tr key={sub.submission_id.clone()}>
                                                    <td>
                                                        { sub.display_name.as_deref().unwrap_or(&sub.username) }
                                                        <br />
                                                        <small>{ &sub.username }</small>
                                                    </td>
                                                    <td>{ &sub.submitted_at[..16] }</td>
                                                    <td>
                                                        if sub.is_late {
                                                            <span class="badge badge-late">{ "Late" }</span>
                                                        } else {
                                                            <span class="badge badge-open">{ "On time" }</span>
                                                        }
                                                    </td>
                                                    <td>{ &sub.method }</td>
                                                    <td>
                                                        <span class={decision_css(&sub.decision)}>
                                                            { decision_label(&sub.decision) }
                                                        </span>
                                                        if sub.decision == "rejected" {
                                                            if let Some(ref r) = sub.reason {
                                                                <br />
                                                                <small class="denial-reason">{ r.as_str() }</small>
                                                            }
                                                        }
                                                    </td>
                                                    <td>
                                                        if sub2.decision == "pending" {
                                                            <button class="btn-sm" onclick={on_review}>
                                                                { "Review" }
                                                            </button>
                                                        }
                                                    </td>
                                                </tr>
                                            }
                                        }) }
                                    </tbody>
                                </table>
                            }
                        </div>
                    }
                </div>
            </div>

            // Decision modal
            if *modal_open {
                <div class="modal-overlay">
                    <div class="modal">
                        <h3>{ "Review Submission" }</h3>

                        if let Some((ok, ref msg)) = *decide_msg {
                            <p class={if ok { "banner-success" } else { "banner-error" }}>
                                { msg.clone() }
                            </p>
                        }

                        <div class="form-row">
                            <label>{ "Decision" }</label>
                            <select
                                value={(*modal_decision).clone()}
                                onchange={{
                                    let md = modal_decision.clone();
                                    Callback::from(move |e: Event| {
                                        let sel: web_sys::HtmlSelectElement = e.target_unchecked_into();
                                        md.set(sel.value());
                                    })
                                }}
                            >
                                <option value="approved">{ "Approve" }</option>
                                <option value="rejected">{ "Deny" }</option>
                            </select>
                        </div>

                        if *modal_decision == "rejected" {
                            <div class="form-row">
                                <label>{ "Reason (required)" }</label>
                                <textarea
                                    value={(*modal_reason).clone()}
                                    oninput={{
                                        let mr = modal_reason.clone();
                                        Callback::from(move |e: InputEvent| {
                                            let ta: web_sys::HtmlTextAreaElement = e.target_unchecked_into();
                                            mr.set(ta.value());
                                        })
                                    }}
                                    placeholder="Enter denial reason\u{2026}"
                                    rows="3"
                                />
                            </div>
                        }

                        <div class="modal-actions">
                            <button
                                class="btn-primary"
                                onclick={on_decide.clone()}
                                disabled={*deciding}
                            >
                                { if *deciding { "Saving\u{2026}" } else { "Confirm" } }
                            </button>
                            <button
                                class="btn-secondary"
                                onclick={{
                                    let mo = modal_open.clone();
                                    Callback::from(move |_: MouseEvent| mo.set(false))
                                }}
                                disabled={*deciding}
                            >
                                { "Cancel" }
                            </button>
                        </div>
                    </div>
                </div>
            }
        </div>
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn status_css(status: &str) -> &'static str {
    match status {
        "open" => "badge badge-open",
        "accepting_late" => "badge badge-late",
        "upcoming" => "badge badge-upcoming",
        _ => "badge badge-closed",
    }
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

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        None => String::new(),
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
    }
}

#[cfg(test)]
mod tests {
    use super::{capitalize, decision_css, decision_label, status_css};

    // ---------------------------------------------------------------------------
    // status_css
    // ---------------------------------------------------------------------------

    #[test]
    fn status_css_open() {
        assert_eq!(status_css("open"), "badge badge-open");
    }

    #[test]
    fn status_css_accepting_late() {
        assert_eq!(status_css("accepting_late"), "badge badge-late");
    }

    #[test]
    fn status_css_upcoming() {
        assert_eq!(status_css("upcoming"), "badge badge-upcoming");
    }

    #[test]
    fn status_css_closed_and_fallback() {
        assert_eq!(status_css("closed"), "badge badge-closed");
        assert_eq!(status_css("unknown"), "badge badge-closed");
        assert_eq!(status_css(""), "badge badge-closed");
    }

    // ---------------------------------------------------------------------------
    // decision_css
    // ---------------------------------------------------------------------------

    #[test]
    fn decision_css_approved() {
        assert_eq!(decision_css("approved"), "decision-badge approved");
    }

    #[test]
    fn decision_css_rejected() {
        assert_eq!(decision_css("rejected"), "decision-badge rejected");
    }

    #[test]
    fn decision_css_pending_and_fallback() {
        assert_eq!(decision_css("pending"), "decision-badge pending");
        assert_eq!(decision_css(""), "decision-badge pending");
    }

    // ---------------------------------------------------------------------------
    // decision_label
    // ---------------------------------------------------------------------------

    #[test]
    fn decision_label_approved() {
        assert_eq!(decision_label("approved"), "Approved");
    }

    #[test]
    fn decision_label_rejected_shows_denied() {
        assert_eq!(decision_label("rejected"), "Denied");
    }

    #[test]
    fn decision_label_pending_and_fallback() {
        assert_eq!(decision_label("pending"), "Pending");
        assert_eq!(decision_label(""), "Pending");
    }

    // ---------------------------------------------------------------------------
    // capitalize
    // ---------------------------------------------------------------------------

    #[test]
    fn capitalize_uppercases_first_char() {
        assert_eq!(capitalize("hello"), "Hello");
    }

    #[test]
    fn capitalize_preserves_rest() {
        assert_eq!(capitalize("openedWindow"), "OpenedWindow");
    }

    #[test]
    fn capitalize_empty_string_returns_empty() {
        assert_eq!(capitalize(""), "");
    }

    #[test]
    fn capitalize_single_char() {
        assert_eq!(capitalize("a"), "A");
    }

    #[test]
    fn capitalize_already_uppercase_unchanged() {
        assert_eq!(capitalize("Approved"), "Approved");
    }
}
