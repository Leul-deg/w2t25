use yew::prelude::*;
use yew_router::prelude::*;
use crate::router::Route;
use crate::state::{AppState, AppStateContext, LockContext};
use crate::components::layout::Layout;
use crate::components::quick_lock::QuickLock;
use crate::pages::login::LoginPage;
use crate::pages::unauthorized::UnauthorizedPage;
use crate::pages::home::HomePage;
use crate::pages::checkin::CheckinPage;
use crate::pages::checkin_review::CheckinReviewPage;
use crate::pages::inbox::InboxPage;
use crate::pages::preferences::PreferencesPage;
use crate::pages::store::StorePage;
use crate::pages::orders::OrdersPage;
use crate::pages::admin_products::AdminProductsPage;
use crate::pages::admin_orders::AdminOrdersPage;
use crate::pages::admin_config::AdminConfigPage;
use crate::pages::admin_kpi::AdminKpiPage;
use crate::pages::admin_reports::AdminReportsPage;
use crate::pages::admin_backups::AdminBackupsPage;
use crate::pages::admin_logs::AdminLogsPage;
use crate::pages::admin_users::AdminUsersPage;
use crate::pages::admin_deletion_requests::AdminDeletionRequestsPage;
use crate::api::auth;

fn is_admin_route(route: &Route) -> bool {
    matches!(
        route,
        Route::Admin
            | Route::AdminUsers
            | Route::AdminDeletionRequests
            | Route::AdminProducts
            | Route::AdminOrders
            | Route::AdminConfig
            | Route::AdminKpi
            | Route::AdminReports
            | Route::AdminBackups
            | Route::AdminLogs
    )
}

fn requires_auth(route: &Route) -> bool {
    !matches!(route, Route::Login | Route::Unauthorized | Route::NotFound)
}

fn switch(route: Route) -> Html {
    match route {
        Route::Login => html! { <LoginPage /> },
        Route::Unauthorized => html! { <UnauthorizedPage /> },

        // ── Admin routes (require Administrator + quick-lock) ─────────────
        Route::Admin
        | Route::AdminUsers
        | Route::AdminDeletionRequests
        | Route::AdminProducts
        | Route::AdminOrders
        | Route::AdminConfig
        | Route::AdminKpi
        | Route::AdminReports
        | Route::AdminBackups
        | Route::AdminLogs => {
            html! {
                <QuickLock>
                    <RoleGuard required_role="Administrator">
                        <AdminShell route={route} />
                    </RoleGuard>
                </QuickLock>
            }
        }

        // ── Store (any authenticated user) ───────────────────────────────
        Route::Store => html! {
            <AuthGuardPage>
                <StorePage />
            </AuthGuardPage>
        },
        Route::Orders => html! {
            <AuthGuardPage>
                <OrdersPage />
            </AuthGuardPage>
        },

        // ── Staff routes ─────────────────────────────────────────────────
        Route::TeacherClasses => {
            html! { <RoleGuard required_role="Teacher"><TeacherClassesShell /></RoleGuard> }
        }

        // ── Check-in ─────────────────────────────────────────────────────
        Route::Checkin => html! { <CheckinPage /> },
        Route::CheckinReview => html! {
            <QuickLock>
                <CheckinReviewPage />
            </QuickLock>
        },

        // ── Common ───────────────────────────────────────────────────────
        Route::Inbox => html! { <InboxPage /> },
        Route::Preferences => html! { <PreferencesPage /> },
        Route::Home | Route::NotFound => html! { <AuthGuard /> },
    }
}

#[cfg(test)]
mod tests {
    use super::{is_admin_route, requires_auth};
    use crate::router::Route;

    #[test]
    fn admin_routes_are_classified_correctly() {
        assert!(is_admin_route(&Route::AdminUsers));
        assert!(is_admin_route(&Route::AdminDeletionRequests));
        assert!(is_admin_route(&Route::AdminReports));
        assert!(!is_admin_route(&Route::Store));
        assert!(!is_admin_route(&Route::Checkin));
    }

    #[test]
    fn auth_requirements_are_classified_correctly() {
        assert!(!requires_auth(&Route::Login));
        assert!(!requires_auth(&Route::Unauthorized));
        assert!(requires_auth(&Route::Store));
        assert!(requires_auth(&Route::AdminLogs));
    }
}

// ---------------------------------------------------------------------------
// AuthGuard — wraps pages that just require authentication
// ---------------------------------------------------------------------------

#[derive(Properties, PartialEq)]
struct AuthGuardPageProps {
    pub children: Children,
}

#[function_component(AuthGuardPage)]
fn auth_guard_page(props: &AuthGuardPageProps) -> Html {
    let state = use_context::<AppStateContext>().expect("AppState context");
    let navigator = use_navigator().unwrap();
    let authenticated = state.is_authenticated();

    {
        let navigator = navigator.clone();
        use_effect_with(authenticated, move |&auth| {
            if !auth {
                navigator.push(&Route::Login);
            }
        });
    }

    if authenticated {
        html! { for props.children.iter() }
    } else {
        html! { <div class="loading">{ "Redirecting\u{2026}" }</div> }
    }
}

// ---------------------------------------------------------------------------
// AuthGuard — redirects to login or shows HomePage
// ---------------------------------------------------------------------------

#[function_component(AuthGuard)]
fn auth_guard() -> Html {
    let state = use_context::<AppStateContext>().expect("AppState context not found");
    let navigator = use_navigator().unwrap();
    let authenticated = state.is_authenticated();

    {
        let navigator = navigator.clone();
        use_effect_with(authenticated, move |&auth| {
            if !auth {
                navigator.push(&Route::Login);
            }
        });
    }

    if authenticated {
        html! { <HomePage /> }
    } else {
        html! { <div class="loading">{ "Redirecting\u{2026}" }</div> }
    }
}

// ---------------------------------------------------------------------------
// RoleGuard
// ---------------------------------------------------------------------------

#[derive(Properties, PartialEq)]
pub struct RoleGuardProps {
    pub required_role: &'static str,
    pub children: Children,
}

#[function_component(RoleGuard)]
fn role_guard(props: &RoleGuardProps) -> Html {
    let state = use_context::<AppStateContext>().expect("AppState context not found");
    let navigator = use_navigator().unwrap();
    let authenticated = state.is_authenticated();
    let has_role = state.has_role(props.required_role);

    let role = props.required_role;
    use_effect_with((authenticated, has_role), move |(auth, role_ok)| {
        if !auth {
            navigator.push(&Route::Login);
        } else if !role_ok {
            navigator.push(&Route::Unauthorized);
        }
        let _ = role;
    });

    if authenticated && has_role {
        html! { for props.children.iter() }
    } else if authenticated && !has_role {
        html! { <UnauthorizedPage /> }
    } else {
        html! { <div class="loading">{ "Checking access\u{2026}" }</div> }
    }
}

// ---------------------------------------------------------------------------
// AdminShell — dispatches admin sub-routes
// ---------------------------------------------------------------------------

#[derive(Properties, PartialEq)]
struct AdminShellProps {
    route: Route,
}

#[function_component(AdminShell)]
fn admin_shell(props: &AdminShellProps) -> Html {
    match props.route {
        Route::AdminProducts => html! { <AdminProductsPage /> },
        Route::AdminOrders   => html! { <AdminOrdersPage /> },
        Route::AdminUsers    => html! { <AdminUsersPage /> },
        Route::AdminDeletionRequests => html! { <AdminDeletionRequestsPage /> },
        Route::AdminConfig   => html! { <AdminConfigPage /> },
        Route::AdminKpi      => html! { <AdminKpiPage /> },
        Route::AdminReports  => html! { <AdminReportsPage /> },
        Route::AdminBackups  => html! { <AdminBackupsPage /> },
        Route::AdminLogs     => html! { <AdminLogsPage /> },
        // Existing admin routes — keep their previous shell content.
        _ => html! {
            <div class="card">
                <h2>{ "Administration" }</h2>
                <p>{ "User management and system settings." }</p>
            </div>
        },
    }
}

#[function_component(TeacherClassesShell)]
fn teacher_classes_shell() -> Html {
    html! {
        <div class="card">
            <h2>{ "My Classes" }</h2>
            <p>{ "Your assigned homerooms and class rosters." }</p>
        </div>
    }
}

// ---------------------------------------------------------------------------
// Root App
// ---------------------------------------------------------------------------

#[function_component(App)]
pub fn app() -> Html {
    let app_state = use_state(AppState::default);
    let lock_state = use_state(|| false);

    {
        let app_state = app_state.clone();
        use_effect_with((), move |_| {
            let app_state = app_state.clone();
            if let Some(token) = (*app_state).token.clone() {
                wasm_bindgen_futures::spawn_local(async move {
                    match auth::me(&token).await {
                        Ok(user) => {
                            let mut new_state = (*app_state).clone();
                            new_state.user = Some(user);
                            app_state.set(new_state);
                        }
                        Err(_) => {
                            let mut new_state = (*app_state).clone();
                            new_state.logout();
                            app_state.set(new_state);
                        }
                    }
                });
            }
        });
    }

    let on_logout = {
        let app_state = app_state.clone();
        let lock_state = lock_state.clone();
        Callback::from(move |_: ()| {
            let token = (*app_state).token.clone();
            let app_state = app_state.clone();
            let lock_state = lock_state.clone();
            wasm_bindgen_futures::spawn_local(async move {
                if let Some(t) = token {
                    let _ = auth::logout(&t).await;
                }
                let mut new_state = (*app_state).clone();
                new_state.logout();
                app_state.set(new_state);
                lock_state.set(false);
            });
        })
    };

    html! {
        <ContextProvider<AppStateContext> context={app_state.clone()}>
            <ContextProvider<LockContext> context={lock_state.clone()}>
                <BrowserRouter>
                    <div id="app">
                        <Layout on_logout={on_logout}>
                            <Switch<Route> render={switch} />
                        </Layout>
                    </div>
                </BrowserRouter>
            </ContextProvider<LockContext>>
        </ContextProvider<AppStateContext>>
    }
}
