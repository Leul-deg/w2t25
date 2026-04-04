use yew::prelude::*;
use crate::state::AppStateContext;

#[function_component(StaffHome)]
pub fn staff_home() -> Html {
    let state = use_context::<AppStateContext>().expect("AppState context not found");
    let username = state
        .user
        .as_ref()
        .map(|u| u.username.as_str())
        .unwrap_or("Staff");

    html! {
        <div>
            <div class="card">
                <span class="role-badge staff">{ "Academic Staff" }</span>
                <h2>{ format!("Welcome, {}!", username) }</h2>
                <p>{ "Academic support operations, reporting, and check-in management." }</p>
            </div>
            <div class="dashboard-grid">
                <div class="dashboard-card">
                    <h3>{ "Check-In Overview" }</h3>
                    <p>{ "Monitor check-in activity across assigned schools." }</p>
                </div>
                <div class="dashboard-card">
                    <h3>{ "Reports" }</h3>
                    <p>{ "Generate and download attendance and activity reports." }</p>
                </div>
                <div class="dashboard-card">
                    <h3>{ "Notifications" }</h3>
                    <p>{ "Manage outgoing notifications and inbox." }</p>
                </div>
            </div>
        </div>
    }
}
