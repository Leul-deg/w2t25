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
    fn admin_dashboard_has_exactly_nine_cards() {
        assert_eq!(admin_dashboard_cards().len(), 9);
    }

    #[test]
    fn admin_dashboard_card_user_management() {
        let cards = admin_dashboard_cards();
        assert!(
            cards.iter().any(|(t, _, r)| *t == "User Management" && *r == Route::AdminUsers),
            "must have User Management → AdminUsers"
        );
    }

    #[test]
    fn admin_dashboard_card_checkin_review() {
        let cards = admin_dashboard_cards();
        assert!(
            cards.iter().any(|(t, _, r)| *t == "Check-In Review" && *r == Route::CheckinReview),
            "must have Check-In Review → CheckinReview"
        );
    }

    #[test]
    fn admin_dashboard_card_products_inventory() {
        let cards = admin_dashboard_cards();
        assert!(
            cards.iter().any(|(t, _, r)| *t == "Products & Inventory" && *r == Route::AdminProducts),
            "must have Products & Inventory → AdminProducts"
        );
    }

    #[test]
    fn admin_dashboard_card_orders_dashboard() {
        let cards = admin_dashboard_cards();
        assert!(
            cards.iter().any(|(t, _, r)| *t == "Orders Dashboard" && *r == Route::AdminOrders),
            "must have Orders Dashboard → AdminOrders"
        );
    }

    #[test]
    fn admin_dashboard_card_configuration() {
        let cards = admin_dashboard_cards();
        assert!(
            cards.iter().any(|(t, _, r)| *t == "Configuration" && *r == Route::AdminConfig),
            "must have Configuration → AdminConfig"
        );
    }

    #[test]
    fn admin_dashboard_card_kpi_dashboard() {
        let cards = admin_dashboard_cards();
        assert!(
            cards.iter().any(|(t, _, r)| *t == "KPI Dashboard" && *r == Route::AdminKpi),
            "must have KPI Dashboard → AdminKpi"
        );
    }

    #[test]
    fn admin_dashboard_card_exports() {
        let cards = admin_dashboard_cards();
        assert!(
            cards.iter().any(|(t, _, r)| *t == "Exports" && *r == Route::AdminReports),
            "must have Exports → AdminReports"
        );
    }

    #[test]
    fn admin_dashboard_card_backups() {
        let cards = admin_dashboard_cards();
        assert!(
            cards.iter().any(|(t, _, r)| *t == "Backups" && *r == Route::AdminBackups),
            "must have Backups → AdminBackups"
        );
    }

    #[test]
    fn admin_dashboard_card_logs() {
        let cards = admin_dashboard_cards();
        assert!(
            cards.iter().any(|(t, _, r)| *t == "Logs" && *r == Route::AdminLogs),
            "must have Logs → AdminLogs"
        );
    }

    #[test]
    fn all_dashboard_cards_have_non_empty_title_and_description() {
        for (title, description, _) in admin_dashboard_cards() {
            assert!(!title.is_empty(), "card title must not be empty");
            assert!(!description.is_empty(), "card description must not be empty");
        }
    }
}
