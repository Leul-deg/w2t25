use wasm_bindgen_futures::spawn_local;
use yew::prelude::*;
use yew_router::prelude::*;

use crate::api::notifications;
use crate::router::Route;
use crate::state::AppStateContext;

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
