use wasm_bindgen_futures::spawn_local;
use yew::prelude::*;
use yew_router::prelude::*;

use crate::api::notifications;
use crate::router::Route;
use crate::state::AppStateContext;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NavTarget {
    Home,
    Store,
    Orders,
    Checkin,
    CheckinReview,
    AdminProducts,
    AdminOrders,
    AdminUsers,
    AdminDeletionRequests,
    AdminConfig,
    AdminKpi,
    AdminReports,
    AdminBackups,
    AdminLogs,
    TeacherClasses,
    Inbox,
    Preferences,
    Login,
}

fn nav_targets_for_roles(is_authenticated: bool, roles: &[String]) -> Vec<NavTarget> {
    if !is_authenticated {
        return vec![NavTarget::Login];
    }

    let has_role = |role: &str| roles.iter().any(|r| r == role);
    let is_admin = has_role("Administrator");
    let is_teacher = has_role("Teacher");
    let is_student = has_role("Student");
    let is_parent = has_role("Parent");
    let is_staff = has_role("AcademicStaff");

    let mut targets = vec![
        NavTarget::Home,
        NavTarget::Store,
        NavTarget::Inbox,
        NavTarget::Preferences,
    ];

    if is_student || is_parent {
        targets.push(NavTarget::Orders);
        targets.push(NavTarget::Checkin);
    }

    if is_admin || is_teacher || is_staff {
        targets.push(NavTarget::CheckinReview);
    }

    if is_admin {
        targets.extend([
            NavTarget::AdminProducts,
            NavTarget::AdminOrders,
            NavTarget::AdminUsers,
            NavTarget::AdminDeletionRequests,
            NavTarget::AdminConfig,
            NavTarget::AdminKpi,
            NavTarget::AdminReports,
            NavTarget::AdminBackups,
            NavTarget::AdminLogs,
        ]);
    }

    if is_teacher {
        targets.push(NavTarget::TeacherClasses);
    }

    targets
}

#[derive(Properties, PartialEq)]
pub struct NavProps {
    pub on_logout: Callback<()>,
}

#[function_component(Nav)]
pub fn nav(props: &NavProps) -> Html {
    let state = use_context::<AppStateContext>().expect("AppState context not found");
    let current_route = use_route::<Route>();
    let on_logout = props.on_logout.clone();
    let unread_count = use_state(|| 0_i64);

    {
        let token = state.token.clone();
        let unread_count = unread_count.clone();
        use_effect_with((token, current_route), move |(token, _route)| {
            let unread_count = unread_count.clone();
            if let Some(token) = token.clone() {
                spawn_local(async move {
                    let unread = notifications::unread_count(&token)
                        .await
                        .map(|resp| resp.unread)
                        .unwrap_or(0);
                    unread_count.set(unread);
                });
            } else {
                unread_count.set(0);
            }
        });
    }

    let handle_logout = Callback::from(move |_: MouseEvent| {
        on_logout.emit(());
    });

    let user_info = if let Some(user) = &state.user {
        let role_display = user.roles.first().map(|r| r.as_str()).unwrap_or("Unknown");
        html! {
            <span class="user-info">
                { format!("{} ({})", user.username, role_display) }
            </span>
        }
    } else {
        html! {}
    };

    let nav_links = if state.is_authenticated() {
        let is_admin   = state.has_role("Administrator");
        let is_teacher = state.has_role("Teacher");
        let is_student = state.has_role("Student");
        let is_parent  = state.has_role("Parent");

        html! {
            <div class="nav-links">
                { user_info }
                <Link<Route> to={Route::Home}>{ "Home" }</Link<Route>>

                // ── Store (students, parents, and anyone authenticated) ───
                <Link<Route> to={Route::Store}>{ "Store" }</Link<Route>>

                if is_student || is_parent {
                    <Link<Route> to={Route::Orders}>{ "My Orders" }</Link<Route>>
                }

                // ── Check-in ─────────────────────────────────────────────
                if is_admin || is_teacher || state.has_role("AcademicStaff") {
                    <Link<Route> to={Route::CheckinReview}>{ "Check-In Review" }</Link<Route>>
                }
                if is_student || is_parent {
                    <Link<Route> to={Route::Checkin}>{ "Check In" }</Link<Route>>
                }

                // ── Admin console ────────────────────────────────────────
                if is_admin {
                    <Link<Route> to={Route::AdminProducts}>{ "Products" }</Link<Route>>
                    <Link<Route> to={Route::AdminOrders}>{ "Orders" }</Link<Route>>
                    <Link<Route> to={Route::AdminUsers}>{ "Users" }</Link<Route>>
                    <Link<Route> to={Route::AdminDeletionRequests}>{ "Deletions" }</Link<Route>>
                    <Link<Route> to={Route::AdminConfig}>{ "Config" }</Link<Route>>
                    <Link<Route> to={Route::AdminKpi}>{ "KPIs" }</Link<Route>>
                    <Link<Route> to={Route::AdminReports}>{ "Reports" }</Link<Route>>
                    <Link<Route> to={Route::AdminBackups}>{ "Backups" }</Link<Route>>
                    <Link<Route> to={Route::AdminLogs}>{ "Logs" }</Link<Route>>
                }

                if is_teacher {
                    <Link<Route> to={Route::TeacherClasses}>{ "My Classes" }</Link<Route>>
                }

                // ── Inbox / Preferences ──────────────────────────────────
                <Link<Route> to={Route::Inbox}>
                    {
                        if *unread_count > 0 {
                            format!("Inbox ({})", *unread_count)
                        } else {
                            "Inbox".to_string()
                        }
                    }
                </Link<Route>>
                <Link<Route> to={Route::Preferences}>{ "Preferences" }</Link<Route>>

                <button onclick={handle_logout}>{ "Sign Out" }</button>
            </div>
        }
    } else {
        html! {
            <div class="nav-links">
                <Link<Route> to={Route::Login}>{ "Sign In" }</Link<Route>>
            </div>
        }
    };

    html! {
        <nav class="meridian-nav">
            <span class="brand">{ "Meridian" }</span>
            { nav_links }
        </nav>
    }
}

#[cfg(test)]
mod tests {
    use super::{nav_targets_for_roles, NavTarget};

    #[test]
    fn unauthenticated_nav_only_shows_login() {
        let targets = nav_targets_for_roles(false, &[]);
        assert_eq!(targets, vec![NavTarget::Login]);
    }

    #[test]
    fn student_nav_includes_orders_and_checkin() {
        let targets = nav_targets_for_roles(true, &[String::from("Student")]);
        assert!(targets.contains(&NavTarget::Orders));
        assert!(targets.contains(&NavTarget::Checkin));
        assert!(!targets.contains(&NavTarget::AdminUsers));
    }

    #[test]
    fn administrator_nav_includes_admin_console_entries() {
        let targets = nav_targets_for_roles(true, &[String::from("Administrator")]);
        assert!(targets.contains(&NavTarget::AdminUsers));
        assert!(targets.contains(&NavTarget::AdminDeletionRequests));
        assert!(targets.contains(&NavTarget::AdminReports));
        assert!(targets.contains(&NavTarget::CheckinReview));
    }

    #[test]
    fn teacher_nav_includes_review_and_classes_only() {
        let targets = nav_targets_for_roles(true, &[String::from("Teacher")]);
        assert!(targets.contains(&NavTarget::CheckinReview));
        assert!(targets.contains(&NavTarget::TeacherClasses));
        assert!(!targets.contains(&NavTarget::Orders));
        assert!(!targets.contains(&NavTarget::AdminUsers));
    }

    #[test]
    fn parent_nav_includes_orders_and_checkin() {
        let targets = nav_targets_for_roles(true, &[String::from("Parent")]);
        assert!(targets.contains(&NavTarget::Orders), "parent must see Orders");
        assert!(targets.contains(&NavTarget::Checkin), "parent must see Checkin");
        assert!(!targets.contains(&NavTarget::AdminUsers), "parent must not see AdminUsers");
        assert!(!targets.contains(&NavTarget::TeacherClasses), "parent must not see TeacherClasses");
    }

    #[test]
    fn academic_staff_nav_includes_checkin_review() {
        let targets = nav_targets_for_roles(true, &[String::from("AcademicStaff")]);
        assert!(targets.contains(&NavTarget::CheckinReview), "staff must see CheckinReview");
        assert!(!targets.contains(&NavTarget::AdminUsers), "staff must not see AdminUsers");
        assert!(!targets.contains(&NavTarget::Orders), "staff must not see Orders");
        assert!(!targets.contains(&NavTarget::TeacherClasses), "staff must not see TeacherClasses");
    }

    #[test]
    fn multi_role_admin_teacher_gets_union_of_both_roles() {
        let targets = nav_targets_for_roles(
            true,
            &[String::from("Administrator"), String::from("Teacher")],
        );
        // Admin entries
        assert!(targets.contains(&NavTarget::AdminUsers));
        assert!(targets.contains(&NavTarget::AdminReports));
        // Teacher entries
        assert!(targets.contains(&NavTarget::TeacherClasses));
        assert!(targets.contains(&NavTarget::CheckinReview));
    }

    #[test]
    fn authenticated_with_no_roles_gets_base_nav_only() {
        // A user with no roles should still get the authenticated base nav
        // (Home, Store, Inbox, Preferences) but no role-specific entries.
        let targets = nav_targets_for_roles(true, &[]);
        assert!(targets.contains(&NavTarget::Home), "base nav must include Home");
        assert!(targets.contains(&NavTarget::Store), "base nav must include Store");
        assert!(targets.contains(&NavTarget::Inbox), "base nav must include Inbox");
        assert!(targets.contains(&NavTarget::Preferences), "base nav must include Preferences");
        assert!(!targets.contains(&NavTarget::AdminUsers), "no roles must not get AdminUsers");
        assert!(!targets.contains(&NavTarget::Orders), "no roles must not get Orders");
        assert!(!targets.contains(&NavTarget::CheckinReview), "no roles must not get CheckinReview");
    }

    #[test]
    fn authenticated_base_nav_always_includes_core_entries() {
        for role in &["Student", "Teacher", "Administrator", "Parent", "AcademicStaff"] {
            let targets = nav_targets_for_roles(true, &[role.to_string()]);
            assert!(targets.contains(&NavTarget::Home),
                "role {} must get Home in nav", role);
            assert!(targets.contains(&NavTarget::Store),
                "role {} must get Store in nav", role);
            assert!(targets.contains(&NavTarget::Inbox),
                "role {} must get Inbox in nav", role);
        }
    }

    #[test]
    fn unauthenticated_never_gets_admin_entries() {
        let targets = nav_targets_for_roles(false, &[String::from("Administrator")]);
        // Even if roles are somehow passed, unauthenticated state must return only Login
        assert_eq!(targets, vec![NavTarget::Login],
            "unauthenticated must see only Login regardless of role list");
    }
}
