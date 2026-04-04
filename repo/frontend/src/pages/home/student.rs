use yew::prelude::*;
use yew_router::prelude::*;
use wasm_bindgen_futures::spawn_local;

use crate::api::checkins;
use crate::router::Route;
use crate::state::AppStateContext;

#[function_component(StudentHome)]
pub fn student_home() -> Html {
    let state = use_context::<AppStateContext>().expect("AppState context not found");
    let username = state
        .user
        .as_ref()
        .map(|u| u.username.as_str())
        .unwrap_or("Student");

    // Find the most relevant open window for the "Check In Now" card.
    let open_window_status = use_state(|| Option::<String>::None); // "open" | "accepting_late" | "upcoming" | "closed"
    let unread_count = use_state(|| 0u64);

    {
        let token = state.token.clone();
        let ows = open_window_status.clone();
        let uc = unread_count.clone();

        use_effect_with((), move |_| {
            let Some(token) = token else { return; };
            let ows2 = ows.clone();
            let uc2 = uc.clone();

            spawn_local(async move {
                // Check for open windows
                if let Ok(windows) = checkins::list_windows(&token).await {
                    let best = windows.iter()
                        .find(|w| w.status == "open")
                        .or_else(|| windows.iter().find(|w| w.status == "accepting_late"))
                        .or_else(|| windows.iter().find(|w| w.status == "upcoming"));
                    ows2.set(best.map(|w| w.status.clone()));
                }

                // Fetch unread count
                if let Ok(c) = crate::api::notifications::unread_count(&token).await {
                    uc2.set(c.unread as u64);
                }
            });
        });
    }

    let checkin_label = match (*open_window_status).as_deref() {
        Some("open") => "Check In Now",
        Some("accepting_late") => "Late Check-In Available",
        Some("upcoming") => "Window Opening Soon",
        _ => "Check-In",
    };

    let checkin_desc = match (*open_window_status).as_deref() {
        Some("open") => "A window is open — tap to check in.",
        Some("accepting_late") => "The window closed but late check-ins are still accepted.",
        Some("upcoming") => "A check-in window will open shortly.",
        _ => "View check-in windows for your school.",
    };

    html! {
        <div>
            <div class="card">
                <span class="role-badge student">{ "Student" }</span>
                <h2>{ format!("Welcome, {}!", username) }</h2>
                <p>{ "View open check-ins, your attendance record, and the school store." }</p>
            </div>
            <div class="dashboard-grid">
                <Link<Route> to={Route::Checkin} classes="dashboard-card dashboard-card-link">
                    <h3>{ checkin_label }</h3>
                    <p>{ checkin_desc }</p>
                    if matches!((*open_window_status).as_deref(), Some("open") | Some("accepting_late")) {
                        <span class="badge badge-open">{ "Action needed" }</span>
                    }
                </Link<Route>>
                <Link<Route> to={Route::Checkin} classes="dashboard-card dashboard-card-link">
                    <h3>{ "My Attendance" }</h3>
                    <p>{ "View your check-in history and approval status." }</p>
                </Link<Route>>
                <Link<Route> to={Route::Store} classes="dashboard-card dashboard-card-link">
                    <h3>{ "School Store" }</h3>
                    <p>{ "Browse and order available merchandise." }</p>
                </Link<Route>>
                <Link<Route> to={Route::Orders} classes="dashboard-card dashboard-card-link">
                    <h3>{ "My Orders" }</h3>
                    <p>{ "Track your order history and fulfillment status." }</p>
                </Link<Route>>
                <Link<Route> to={Route::Inbox} classes="dashboard-card dashboard-card-link">
                    <h3>
                        { "Inbox" }
                        if *unread_count > 0 {
                            <span class="unread-badge">{ format!(" {}", *unread_count) }</span>
                        }
                    </h3>
                    <p>{ "Check-in results and school notifications." }</p>
                </Link<Route>>
            </div>
        </div>
    }
}
