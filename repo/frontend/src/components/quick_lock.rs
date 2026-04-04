/// Quick-lock overlay for sensitive screens.
///
/// Wraps its children in an activity-tracking div. After 10 minutes of no
/// mouse or keyboard activity the screen is locked and a password prompt is
/// shown. The user must re-enter their password (verified via POST /auth/verify)
/// to resume. The lock state is held in `LockContext` — provided at App root —
/// so navigating between wrapped routes does not reset the inactivity timer.
///
/// Critical rule: locking is not cosmetic. The children are removed from the
/// DOM while locked; they are not merely hidden with CSS.
use std::cell::RefCell;
use std::rc::Rc;

use wasm_bindgen_futures::spawn_local;
use yew::prelude::*;
use yew_router::prelude::*;

use crate::api::auth;
use crate::api::client::ApiError;
use crate::router::Route;
use crate::state::{AppStateContext, LockContext};

const INACTIVITY_MS: f64 = 10.0 * 60.0 * 1000.0; // 10 minutes
const CHECK_INTERVAL_MS: u32 = 30_000;             // check every 30 s

#[derive(Properties, PartialEq)]
pub struct QuickLockProps {
    pub children: Children,
}

#[function_component(QuickLock)]
pub fn quick_lock(props: &QuickLockProps) -> Html {
    let locked = use_context::<LockContext>().expect("LockContext missing — wrap App in ContextProvider<LockContext>");
    let app_state = use_context::<AppStateContext>().expect("AppStateContext missing");
    let navigator = use_navigator().unwrap();

    // Shared mutable timestamp: last recorded user activity.
    let last_activity: Rc<RefCell<f64>> = use_mut_ref(|| js_sys::Date::now());

    // Password input / unlock UI state.
    let password = use_state(String::new);
    let unlock_error = use_state(|| Option::<String>::None);
    let verifying = use_state(|| false);

    // ── Inactivity check ─────────────────────────────────────────────────
    {
        let locked = locked.clone();
        let last = last_activity.clone();
        use_effect_with((), move |_| {
            let interval = gloo_timers::callback::Interval::new(CHECK_INTERVAL_MS, move || {
                let elapsed = js_sys::Date::now() - *last.borrow();
                if elapsed >= INACTIVITY_MS {
                    locked.set(true);
                }
            });
            // Drop the interval when the component unmounts.
            move || drop(interval)
        });
    }

    // ── Activity callbacks ────────────────────────────────────────────────
    let on_mouse = {
        let last = last_activity.clone();
        Callback::from(move |_: MouseEvent| {
            *last.borrow_mut() = js_sys::Date::now();
        })
    };

    let on_key = {
        let last = last_activity.clone();
        Callback::from(move |_: KeyboardEvent| {
            *last.borrow_mut() = js_sys::Date::now();
        })
    };

    // ── Unlock handler ────────────────────────────────────────────────────
    let on_password_input = {
        let password = password.clone();
        Callback::from(move |e: InputEvent| {
            let input: web_sys::HtmlInputElement = e.target_unchecked_into();
            password.set(input.value());
        })
    };

    let on_unlock = {
        let password = password.clone();
        let locked = locked.clone();
        let unlock_error = unlock_error.clone();
        let verifying = verifying.clone();
        let token = app_state.token.clone();
        let app_state = app_state.clone();
        let navigator = navigator.clone();
        let last_activity = last_activity.clone();

        Callback::from(move |e: MouseEvent| {
            e.prevent_default();
            if *verifying {
                return;
            }

            let pw = (*password).clone();
            if pw.is_empty() {
                unlock_error.set(Some("Please enter your password.".to_string()));
                return;
            }

            let locked = locked.clone();
            let unlock_error = unlock_error.clone();
            let verifying = verifying.clone();
            let password = password.clone();
            let token = token.clone();
            let app_state = app_state.clone();
            let navigator = navigator.clone();
            let last_activity = last_activity.clone();

            verifying.set(true);
            unlock_error.set(None);

            spawn_local(async move {
                let result =
                    auth::verify_password(&pw, token.as_deref().unwrap_or("")).await;
                match result {
                    Ok(()) => {
                        password.set(String::new());
                        *last_activity.borrow_mut() = js_sys::Date::now();
                        locked.set(false);
                    }
                    Err(ApiError::Http { status: 403, .. }) => {
                        let mut new_state = (*app_state).clone();
                        new_state.logout();
                        app_state.set(new_state);
                        locked.set(false);
                        navigator.push(&Route::Login);
                    }
                    Err(ApiError::Http { status: 401, message }) => {
                        let msg = serde_json::from_str::<serde_json::Value>(&message)
                            .ok()
                            .and_then(|v| v["error"].as_str().map(|s| s.to_string()))
                            .unwrap_or_else(|| {
                                "Incorrect password or session expired. Please try again.".to_string()
                            });
                        unlock_error.set(Some(msg));
                    }
                    Err(ApiError::Http { status: 429, message }) => {
                        let msg = serde_json::from_str::<serde_json::Value>(&message)
                            .ok()
                            .and_then(|v| v["error"].as_str().map(|s| s.to_string()))
                            .unwrap_or_else(|| "Too many unlock attempts. Please wait and try again.".to_string());
                        unlock_error.set(Some(msg));
                    }
                    Err(ApiError::Http { message, .. }) => {
                        let msg = serde_json::from_str::<serde_json::Value>(&message)
                            .ok()
                            .and_then(|v| v["error"].as_str().map(|s| s.to_string()))
                            .unwrap_or_else(|| "Incorrect password. Please try again.".to_string());
                        unlock_error.set(Some(msg));
                    }
                    Err(_) => {
                        unlock_error.set(Some("Unable to verify your session right now. Please try again.".to_string()));
                    }
                }
                verifying.set(false);
            });
        })
    };

    // ── Render ────────────────────────────────────────────────────────────
    if *locked {
        // Children are NOT rendered while locked — removed from DOM entirely.
        html! {
            <div class="lock-overlay">
                <div class="lock-card">
                    <div class="lock-icon">{ "\u{1F512}" }</div>
                    <h2>{ "Screen Locked" }</h2>
                    <p class="lock-subtitle">
                        { "Your session was locked due to inactivity. Enter your password to continue." }
                    </p>
                    <div class="lock-form">
                        <input
                            type="password"
                            class="lock-input"
                            placeholder="Password"
                            value={(*password).clone()}
                            oninput={on_password_input}
                            disabled={*verifying}
                        />
                        if let Some(ref err) = *unlock_error {
                            <p class="error-msg">{ err.clone() }</p>
                        }
                        <button
                            class="btn-primary lock-btn"
                            onclick={on_unlock}
                            disabled={*verifying}
                        >
                            { if *verifying { "Verifying\u{2026}" } else { "Unlock" } }
                        </button>
                    </div>
                </div>
            </div>
        }
    } else {
        html! {
            <div
                onmousemove={on_mouse.clone()}
                onclick={on_mouse}
                onkeydown={on_key}
                style="min-height: 100%"
            >
                { for props.children.iter() }
            </div>
        }
    }
}
