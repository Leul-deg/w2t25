use yew::prelude::*;
use yew_router::prelude::*;
use crate::router::Route;
use crate::state::{AppStateContext, LoginError};
use crate::api::auth;

#[function_component(LoginPage)]
pub fn login_page() -> Html {
    let state = use_context::<AppStateContext>().expect("AppState context not found");
    let navigator = use_navigator().unwrap();

    let username = use_state(String::new);
    let password = use_state(String::new);
    let login_error = use_state(|| Option::<LoginError>::None);
    let loading = use_state(|| false);

    // Redirect if already authenticated
    {
        let state = state.clone();
        let navigator = navigator.clone();
        use_effect_with(state.clone(), move |state| {
            if state.is_authenticated() {
                navigator.push(&Route::Home);
            }
        });
    }

    let on_username_input = {
        let username = username.clone();
        Callback::from(move |e: InputEvent| {
            let input: web_sys::HtmlInputElement = e.target_unchecked_into();
            username.set(input.value());
        })
    };

    let on_password_input = {
        let password = password.clone();
        Callback::from(move |e: InputEvent| {
            let input: web_sys::HtmlInputElement = e.target_unchecked_into();
            password.set(input.value());
        })
    };

    let is_form_blocked = matches!(
        *login_error,
        Some(LoginError::TooManyAttempts(_)) | Some(LoginError::AccountBlocked(_))
    );

    let on_submit = {
        let username = username.clone();
        let password = password.clone();
        let login_error = login_error.clone();
        let loading = loading.clone();
        let state = state.clone();
        let navigator = navigator.clone();

        Callback::from(move |e: SubmitEvent| {
            e.prevent_default();

            let u = (*username).clone();
            let p = (*password).clone();

            if u.is_empty() || p.is_empty() {
                login_error.set(Some(LoginError::ValidationError(
                    "Username and password are required.".into(),
                )));
                return;
            }

            let login_error = login_error.clone();
            let loading = loading.clone();
            let state = state.clone();
            let navigator = navigator.clone();

            loading.set(true);
            login_error.set(None);

            wasm_bindgen_futures::spawn_local(async move {
                match auth::login(u, p).await {
                    Ok(resp) => {
                        let mut new_state = (*state).clone();
                        new_state.login(resp.token, resp.user);
                        state.set(new_state);
                        navigator.push(&Route::Home);
                    }
                    Err(err) => {
                        login_error.set(Some(err));
                        loading.set(false);
                    }
                }
            });
        })
    };

    let error_banner = if let Some(ref err) = *login_error {
        let css = format!("login-error {}", err.css_class());
        let icon = match err {
            LoginError::TooManyAttempts(_) => "\u{23F3}",
            LoginError::AccountBlocked(_) => "\u{1F6AB}",
            LoginError::InvalidCredentials => "\u{26A0}",
            LoginError::ValidationError(_) => "\u{2139}",
            LoginError::NetworkError(_) => "\u{26A1}",
        };
        html! {
            <div class={css}>
                <span class="error-icon">{ icon }</span>
                <span>{ err.display_message() }</span>
            </div>
        }
    } else {
        html! {}
    };

    let button_text = if *loading { "Signing in\u{2026}" } else { "Sign In" };

    html! {
        <div class="login-container">
            <div class="login-box">
                <h1>{ "Meridian" }</h1>
                <p class="subtitle">{ "Check-In & Commerce Operations Suite" }</p>
                <form onsubmit={on_submit}>
                    <div class="form-group">
                        <label for="username">{ "Username" }</label>
                        <input
                            id="username"
                            type="text"
                            placeholder="Enter your username"
                            value={(*username).clone()}
                            oninput={on_username_input}
                            autocomplete="username"
                            disabled={*loading || is_form_blocked}
                        />
                    </div>
                    <div class="form-group">
                        <label for="password">{ "Password" }</label>
                        <input
                            id="password"
                            type="password"
                            placeholder="Enter your password"
                            value={(*password).clone()}
                            oninput={on_password_input}
                            autocomplete="current-password"
                            disabled={*loading || is_form_blocked}
                        />
                    </div>
                    { error_banner }
                    <button class="btn-primary" type="submit"
                        disabled={*loading || is_form_blocked}>
                        { button_text }
                    </button>
                </form>
            </div>
        </div>
    }
}
