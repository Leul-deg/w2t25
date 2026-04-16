use wasm_bindgen_futures::spawn_local;
use web_sys::{HtmlInputElement, HtmlSelectElement};
use yew::prelude::*;

use crate::api::admin_ops::{self, AdminUser, SetUserStateRequest};
use crate::state::AppStateContext;

fn available_account_states() -> [&'static str; 4] {
    ["active", "disabled", "frozen", "blacklisted"]
}

fn normalize_optional_reason(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

#[function_component(AdminUsersPage)]
pub fn admin_users_page() -> Html {
    let state = use_context::<AppStateContext>().expect("AppState context");
    let users = use_state(Vec::<AdminUser>::new);
    let loading = use_state(|| true);
    let error = use_state(String::new);
    let success = use_state(String::new);
    let state_edits: UseStateHandle<std::collections::HashMap<String, String>> =
        use_state(std::collections::HashMap::new);
    let reason_edits: UseStateHandle<std::collections::HashMap<String, String>> =
        use_state(std::collections::HashMap::new);

    let reload = {
        let users = users.clone();
        let loading = loading.clone();
        let error = error.clone();
        let token = state.token.clone();
        Callback::from(move |_: ()| {
            let users = users.clone();
            let loading = loading.clone();
            let error = error.clone();
            let token = token.clone();
            loading.set(true);
            if let Some(token) = token {
                spawn_local(async move {
                    match admin_ops::list_admin_users(&token).await {
                        Ok(rows) => {
                            users.set(rows);
                            error.set(String::new());
                        }
                        Err(e) => error.set(format!("Failed to load users: {}", e)),
                    }
                    loading.set(false);
                });
            }
        })
    };

    {
        let reload = reload.clone();
        use_effect_with((), move |_| reload.emit(()));
    }

    html! {
        <div class="page-container">
            <h2>{ "User Management" }</h2>
            if !error.is_empty() { <div class="error-banner">{ (*error).clone() }</div> }
            if !success.is_empty() { <div class="success-banner">{ (*success).clone() }</div> }
            if *loading {
                <div class="card"><p>{ "Loading users..." }</p></div>
            } else {
                <table class="data-table">
                    <thead>
                        <tr>
                            <th>{ "Username" }</th>
                            <th>{ "Email" }</th>
                            <th>{ "Roles" }</th>
                            <th>{ "State" }</th>
                            <th>{ "New State" }</th>
                            <th>{ "Reason" }</th>
                            <th>{ "Apply" }</th>
                        </tr>
                    </thead>
                    <tbody>
                        { for users.iter().map(|user| {
                            let user_id = user.id.clone();
                            let selected_state = state_edits.get(&user_id).cloned().unwrap_or_else(|| user.account_state.clone());
                            let reason_value = reason_edits.get(&user_id).cloned().unwrap_or_default();

                            let on_state_change = {
                                let state_edits = state_edits.clone();
                                let user_id = user_id.clone();
                                Callback::from(move |e: Event| {
                                    let el: HtmlSelectElement = e.target_unchecked_into();
                                    let mut map = (*state_edits).clone();
                                    map.insert(user_id.clone(), el.value());
                                    state_edits.set(map);
                                })
                            };

                            let on_reason_input = {
                                let reason_edits = reason_edits.clone();
                                let user_id = user_id.clone();
                                Callback::from(move |e: InputEvent| {
                                    let el: HtmlInputElement = e.target_unchecked_into();
                                    let mut map = (*reason_edits).clone();
                                    map.insert(user_id.clone(), el.value());
                                    reason_edits.set(map);
                                })
                            };

                            let on_apply = {
                                let token = state.token.clone();
                                let state_edits = state_edits.clone();
                                let reason_edits = reason_edits.clone();
                                let reload = reload.clone();
                                let error = error.clone();
                                let success = success.clone();
                                let user_id = user_id.clone();
                                let username = user.username.clone();
                                Callback::from(move |_: MouseEvent| {
                                    if let Some(token) = token.clone() {
                                        let next_state = state_edits
                                            .get(&user_id)
                                            .cloned()
                                            .unwrap_or_else(|| "active".to_string());
                                        let reason = reason_edits
                                            .get(&user_id)
                                            .and_then(|s| normalize_optional_reason(s));
                                        let req = SetUserStateRequest { state: next_state, reason };
                                        let reload = reload.clone();
                                        let error = error.clone();
                                        let success = success.clone();
                                        let user_id = user_id.clone();
                                        let username = username.clone();
                                        spawn_local(async move {
                                            match admin_ops::set_admin_user_state(&token, &user_id, &req).await {
                                                Ok(_) => {
                                                    success.set(format!("Updated state for '{}'.", username));
                                                    error.set(String::new());
                                                    reload.emit(());
                                                }
                                                Err(e) => error.set(format!("Failed to update user state: {}", e)),
                                            }
                                        });
                                    }
                                })
                            };

                            html! {
                                <tr key={user.id.clone()}>
                                    <td>{ &user.username }</td>
                                    <td>{ &user.email }</td>
                                    <td>{ user.roles.join(", ") }</td>
                                    <td>{ &user.account_state }</td>
                                    <td>
                                        <select value={selected_state} onchange={on_state_change}>
                                            { for available_account_states().into_iter().map(|state_name| html! {
                                                <option value={state_name}>{ state_name }</option>
                                            }) }
                                        </select>
                                    </td>
                                    <td>
                                        <input class="inline-input" type="text" value={reason_value} oninput={on_reason_input} placeholder="Optional reason" />
                                    </td>
                                    <td>
                                        <button class="btn-sm btn-primary" onclick={on_apply}>{ "Apply" }</button>
                                    </td>
                                </tr>
                            }
                        }) }
                    </tbody>
                </table>
            }
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::{available_account_states, normalize_optional_reason};

    // -----------------------------------------------------------------------
    // available_account_states
    // -----------------------------------------------------------------------

    #[test]
    fn account_states_has_exactly_four_entries() {
        assert_eq!(available_account_states().len(), 4);
    }

    #[test]
    fn account_states_match_backend_contract() {
        let states = available_account_states();
        assert_eq!(states, ["active", "disabled", "frozen", "blacklisted"]);
    }

    #[test]
    fn account_states_contains_active() {
        assert!(available_account_states().contains(&"active"));
    }

    #[test]
    fn account_states_contains_disabled() {
        assert!(available_account_states().contains(&"disabled"));
    }

    #[test]
    fn account_states_contains_frozen() {
        assert!(available_account_states().contains(&"frozen"));
    }

    #[test]
    fn account_states_contains_blacklisted() {
        assert!(available_account_states().contains(&"blacklisted"));
    }

    #[test]
    fn account_states_first_entry_is_active() {
        // "active" is the default UI state — must be first so it pre-selects correctly.
        assert_eq!(available_account_states()[0], "active");
    }

    // -----------------------------------------------------------------------
    // normalize_optional_reason
    // -----------------------------------------------------------------------

    #[test]
    fn empty_string_reason_is_none() {
        assert_eq!(normalize_optional_reason(""), None);
    }

    #[test]
    fn whitespace_only_reason_is_none() {
        assert_eq!(normalize_optional_reason("   "), None);
        assert_eq!(normalize_optional_reason("\t\n"), None);
    }

    #[test]
    fn empty_reason_is_none() {
        assert_eq!(normalize_optional_reason(""), None);
        assert_eq!(normalize_optional_reason("   "), None);
    }

    #[test]
    fn non_empty_reason_is_returned_as_some() {
        assert!(normalize_optional_reason("policy violation").is_some());
    }

    #[test]
    fn non_empty_reason_is_trimmed() {
        assert_eq!(
            normalize_optional_reason("  policy violation "),
            Some("policy violation".to_string())
        );
    }

    #[test]
    fn reason_with_leading_whitespace_is_trimmed() {
        assert_eq!(
            normalize_optional_reason("   repeated absence"),
            Some("repeated absence".to_string())
        );
    }

    #[test]
    fn reason_with_trailing_whitespace_is_trimmed() {
        assert_eq!(
            normalize_optional_reason("repeated absence   "),
            Some("repeated absence".to_string())
        );
    }

    #[test]
    fn single_word_reason_is_preserved() {
        assert_eq!(
            normalize_optional_reason("blacklisted"),
            Some("blacklisted".to_string())
        );
    }
}
