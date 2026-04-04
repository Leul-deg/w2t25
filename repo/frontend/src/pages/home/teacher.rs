use yew::prelude::*;
use yew_router::prelude::*;
use wasm_bindgen_futures::spawn_local;

use crate::api::checkins::{self, SubmissionFilters};
use crate::router::Route;
use crate::state::AppStateContext;

#[function_component(TeacherHome)]
pub fn teacher_home() -> Html {
    let state = use_context::<AppStateContext>().expect("AppState context not found");
    let username = state
        .user
        .as_ref()
        .map(|u| u.username.as_str())
        .unwrap_or("Teacher");

    let pending_count = use_state(|| 0usize);
    let unread_count = use_state(|| 0u64);

    {
        let token = state.token.clone();
        let pc = pending_count.clone();
        let uc = unread_count.clone();

        use_effect_with((), move |_| {
            let Some(token) = token else { return; };
            let pc2 = pc.clone();
            let uc2 = uc.clone();

            spawn_local(async move {
                // Count pending submissions across visible windows.
                // We fetch windows then for each open/accepting_late window
                // count pending submissions. This is an approximation —
                // listing all windows and checking each one would be expensive.
                // Instead we surface the badge from a single windows call.
                if let Ok(windows) = checkins::list_windows(&token).await {
                    let active: Vec<_> = windows.into_iter()
                        .filter(|w| w.status == "open" || w.status == "accepting_late")
                        .collect();

                    let mut total_pending = 0usize;
                    for win in &active {
                        let filters = SubmissionFilters {
                                school_id: String::new(),
                                decision: String::new(),
                                homeroom_id: String::new(),
                                date_from: String::new(),
                                date_to: String::new(),
                            };
                            if let Ok(subs) = checkins::list_submissions(&win.id, &filters, &token).await {
                            total_pending += subs.iter()
                                .filter(|s| s.decision == "pending")
                                .count();
                        }
                    }
                    pc2.set(total_pending);
                }

                if let Ok(c) = crate::api::notifications::unread_count(&token).await {
                    uc2.set(c.unread as u64);
                }
            });
        });
    }

    html! {
        <div>
            <div class="card">
                <span class="role-badge teacher">{ "Teacher" }</span>
                <h2>{ format!("Welcome, {}!", username) }</h2>
                <p>{ "Manage your classes, review check-ins, and view student activity." }</p>
            </div>
            <div class="dashboard-grid">
                <div class="dashboard-card">
                    <h3>{ "My Classes" }</h3>
                    <p>{ "View your assigned homerooms and class rosters." }</p>
                </div>
                <Link<Route> to={Route::CheckinReview} classes="dashboard-card dashboard-card-link">
                    <h3>
                        { "Check-In Review" }
                        if *pending_count > 0 {
                            <span class="unread-badge">{ format!(" {} pending", *pending_count) }</span>
                        }
                    </h3>
                    <p>{ "Review and approve pending check-in submissions." }</p>
                </Link<Route>>
                <Link<Route> to={Route::Inbox} classes="dashboard-card dashboard-card-link">
                    <h3>
                        { "Notifications" }
                        if *unread_count > 0 {
                            <span class="unread-badge">{ format!(" {}", *unread_count) }</span>
                        }
                    </h3>
                    <p>{ "View messages and alerts in your inbox." }</p>
                </Link<Route>>
            </div>
        </div>
    }
}
