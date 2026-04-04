use yew::prelude::*;
use yew_router::prelude::*;

use crate::router::Route;
use crate::state::AppStateContext;

#[function_component(AdminHome)]
pub fn admin_home() -> Html {
    let state = use_context::<AppStateContext>().expect("AppState context not found");
    let username = state
        .user
        .as_ref()
        .map(|u| u.username.as_str())
        .unwrap_or("Admin");

    html! {
        <div>
            <div class="card">
                <span class="role-badge admin">{ "Administrator" }</span>
                <h2>{ format!("Welcome, {}!", username) }</h2>
                <p>{ "Full system access. Manage users, districts, check-ins, commerce, and reporting." }</p>
            </div>
            <div class="dashboard-grid">
                <Link<Route> to={Route::AdminUsers} classes="dashboard-card dashboard-card-link">
                    <h3>{ "User Management" }</h3>
                    <p>{ "Create, edit, and manage accounts across all roles." }</p>
                </Link<Route>>
                <Link<Route> to={Route::CheckinReview} classes="dashboard-card dashboard-card-link">
                    <h3>{ "Check-In Windows" }</h3>
                    <p>{ "Create and monitor check-in windows across all schools." }</p>
                </Link<Route>>
                <Link<Route> to={Route::AdminProducts} classes="dashboard-card dashboard-card-link">
                    <h3>{ "Commerce" }</h3>
                    <p>{ "Manage products, inventory, and order fulfillment." }</p>
                </Link<Route>>
                <Link<Route> to={Route::AdminOrders} classes="dashboard-card dashboard-card-link">
                    <h3>{ "Reports" }</h3>
                    <p>{ "Monitor the live order dashboard and order details." }</p>
                </Link<Route>>
                <Link<Route> to={Route::AdminConfig} classes="dashboard-card dashboard-card-link">
                    <h3>{ "Configuration" }</h3>
                    <p>{ "System settings, campaign toggles, and backup management." }</p>
                </Link<Route>>
                <Link<Route> to={Route::AdminKpi} classes="dashboard-card dashboard-card-link">
                    <h3>{ "Audit Logs" }</h3>
                    <p>{ "Review KPIs, operational metrics, and low-stock signals." }</p>
                </Link<Route>>
                <Link<Route> to={Route::AdminReports} classes="dashboard-card dashboard-card-link">
                    <h3>{ "Exports" }</h3>
                    <p>{ "Generate CSV reports with masked or permission-controlled PII." }</p>
                </Link<Route>>
                <Link<Route> to={Route::AdminBackups} classes="dashboard-card dashboard-card-link">
                    <h3>{ "Backups" }</h3>
                    <p>{ "Create encrypted backups and prepare validated restores." }</p>
                </Link<Route>>
                <Link<Route> to={Route::AdminLogs} classes="dashboard-card dashboard-card-link">
                    <h3>{ "Logs" }</h3>
                    <p>{ "Inspect audit, access, and error logs with retention controls." }</p>
                </Link<Route>>
            </div>
        </div>
    }
}
