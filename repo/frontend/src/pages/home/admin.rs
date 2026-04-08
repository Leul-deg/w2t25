use yew::prelude::*;
use yew_router::prelude::*;

use crate::router::Route;
use crate::state::AppStateContext;

fn admin_dashboard_cards() -> Vec<(&'static str, &'static str, Route)> {
    vec![
        ("User Management", "Create, edit, and manage accounts across all roles.", Route::AdminUsers),
        ("Check-In Review", "Review submissions and monitor check-in activity across schools.", Route::CheckinReview),
        ("Products & Inventory", "Manage catalog items, stock levels, and low-stock thresholds.", Route::AdminProducts),
        ("Orders Dashboard", "Monitor live order operations, detail, and status transitions.", Route::AdminOrders),
        ("Configuration", "System settings, campaign toggles, and backup management.", Route::AdminConfig),
        ("KPI Dashboard", "Review sales, average order value, repeat purchase rate, and metrics.", Route::AdminKpi),
        ("Exports", "Generate CSV reports with masked or permission-controlled PII.", Route::AdminReports),
        ("Backups", "Create encrypted backups and prepare validated restores.", Route::AdminBackups),
        ("Logs", "Inspect audit, access, and error logs with retention controls.", Route::AdminLogs),
    ]
}

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
                { for admin_dashboard_cards().into_iter().map(|(title, description, route)| html! {
                    <Link<Route> to={route} classes="dashboard-card dashboard-card-link">
                        <h3>{ title }</h3>
                        <p>{ description }</p>
                    </Link<Route>>
                }) }
            </div>
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::admin_dashboard_cards;
    use crate::router::Route;

    #[test]
    fn admin_dashboard_cards_have_expected_routes() {
        let cards = admin_dashboard_cards();
        assert!(cards.iter().any(|(title, _, route)| *title == "User Management" && *route == Route::AdminUsers));
        assert!(cards.iter().any(|(title, _, route)| *title == "Orders Dashboard" && *route == Route::AdminOrders));
        assert!(cards.iter().any(|(title, _, route)| *title == "Exports" && *route == Route::AdminReports));
    }
}
