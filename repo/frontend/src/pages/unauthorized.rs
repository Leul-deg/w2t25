use yew::prelude::*;
use yew_router::prelude::*;
use crate::router::Route;

#[function_component(UnauthorizedPage)]
pub fn unauthorized_page() -> Html {
    let navigator = use_navigator().unwrap();
    let go_home = {
        let navigator = navigator.clone();
        Callback::from(move |_: MouseEvent| {
            navigator.push(&Route::Home);
        })
    };
    let go_login = Callback::from(move |_: MouseEvent| {
        navigator.push(&Route::Login);
    });

    html! {
        <div class="unauthorized-container">
            <div class="unauthorized-icon">{ "\u{1F512}" }</div>
            <h1>{ "Access Denied" }</h1>
            <p>{ "You do not have permission to view this page. If you believe this is an error, contact your administrator." }</p>
            <div style="display:flex;gap:0.75rem;justify-content:center;flex-wrap:wrap;">
                <button class="btn-link" onclick={go_home}>{ "\u{2190} Back to Home" }</button>
                <button class="btn-link" onclick={go_login}>{ "Sign In with Different Account" }</button>
            </div>
        </div>
    }
}
