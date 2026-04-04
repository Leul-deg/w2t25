use yew::prelude::*;
use yew_router::prelude::*;
use wasm_bindgen_futures::spawn_local;

use crate::api::checkins;
use crate::router::Route;
use crate::state::AppStateContext;

#[function_component(ParentHome)]
pub fn parent_home() -> Html {
    let state = use_context::<AppStateContext>().expect("AppState context not found");
    let username = state
        .user
        .as_ref()
        .map(|u| u.username.as_str())
        .unwrap_or("Parent");

    let open_window_exists = use_state(|| false);
    let unread_count = use_state(|| 0u64);

    {
        let token = state.token.clone();
        let owe = open_window_exists.clone();
        let uc = unread_count.clone();

        use_effect_with((), move |_| {
            let Some(token) = token else { return; };
            let owe2 = owe.clone();
            let uc2 = uc.clone();

            spawn_local(async move {
                if let Ok(windows) = checkins::list_windows(&token).await {
                    let has_open = windows.iter().any(|w| {
                        w.status == "open" || w.status == "accepting_late"
                    });
                    owe2.set(has_open);
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
                <span class="role-badge parent">{ "Parent / Guardian" }</span>
                <h2>{ format!("Welcome, {}!", username) }</h2>
                <p>{ "View your child's check-in history, orders, and school notifications." }</p>
            </div>
            <div class="dashboard-grid">
                <Link<Route> to={Route::Checkin} classes="dashboard-card dashboard-card-link">
                    <h3>{ "Check In" }</h3>
                    <p>{ "Check in for your linked students." }</p>
                    if *open_window_exists {
                        <span class="badge badge-open">{ "Window open" }</span>
                    }
                </Link<Route>>
                <Link<Route> to={Route::Checkin} classes="dashboard-card dashboard-card-link">
                    <h3>{ "Check-In History" }</h3>
                    <p>{ "Review recent check-in records and approval status." }</p>
                </Link<Route>>
                <Link<Route> to={Route::Orders} classes="dashboard-card dashboard-card-link">
                    <h3>{ "Orders" }</h3>
                    <p>{ "Track merchandise orders and purchase history." }</p>
                </Link<Route>>
                <Link<Route> to={Route::Inbox} classes="dashboard-card dashboard-card-link">
                    <h3>
                        { "Notifications" }
                        if *unread_count > 0 {
                            <span class="unread-badge">{ format!(" {}", *unread_count) }</span>
                        }
                    </h3>
                    <p>{ "School announcements and check-in alerts." }</p>
                </Link<Route>>
            </div>
        </div>
    }
}
