use wasm_bindgen_futures::spawn_local;
use web_sys::HtmlInputElement;
use yew::prelude::*;

use crate::api::admin_ops::{self, DeletionRequestRow, RejectDeletionRequest};
use crate::state::AppStateContext;

fn normalize_rejection_reason(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

#[function_component(AdminDeletionRequestsPage)]
pub fn admin_deletion_requests_page() -> Html {
    let state = use_context::<AppStateContext>().expect("AppState context");
    let requests = use_state(Vec::<DeletionRequestRow>::new);
    let loading = use_state(|| true);
    let error = use_state(String::new);
    let success = use_state(String::new);
    let rejection_reasons: UseStateHandle<std::collections::HashMap<String, String>> =
        use_state(std::collections::HashMap::new);

    let reload = {
        let requests = requests.clone();
        let loading = loading.clone();
        let error = error.clone();
        let token = state.token.clone();
        Callback::from(move |_: ()| {
            let requests = requests.clone();
            let loading = loading.clone();
            let error = error.clone();
            let token = token.clone();
            loading.set(true);
            if let Some(token) = token {
                spawn_local(async move {
                    match admin_ops::list_deletion_requests(&token).await {
                        Ok(rows) => {
                            requests.set(rows);
                            error.set(String::new());
                        }
                        Err(e) => error.set(format!("Failed to load deletion requests: {}", e)),
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
            <h2>{ "Deletion Requests" }</h2>
            if !error.is_empty() { <div class="error-banner">{ (*error).clone() }</div> }
            if !success.is_empty() { <div class="success-banner">{ (*success).clone() }</div> }
            if *loading {
                <div class="card"><p>{ "Loading deletion requests..." }</p></div>
            } else if requests.is_empty() {
                <div class="card"><p>{ "No pending deletion requests." }</p></div>
            } else {
                <table class="data-table">
                    <thead>
                        <tr>
                            <th>{ "User" }</th>
                            <th>{ "Email" }</th>
                            <th>{ "Reason" }</th>
                            <th>{ "Requested" }</th>
                            <th>{ "Reject Reason" }</th>
                            <th>{ "Actions" }</th>
                        </tr>
                    </thead>
                    <tbody>
                        { for requests.iter().map(|req| {
                            let request_id = req.id.clone();
                            let reject_reason = rejection_reasons.get(&request_id).cloned().unwrap_or_default();
                            let on_reason_input = {
                                let rejection_reasons = rejection_reasons.clone();
                                let request_id = request_id.clone();
                                Callback::from(move |e: InputEvent| {
                                    let el: HtmlInputElement = e.target_unchecked_into();
                                    let mut map = (*rejection_reasons).clone();
                                    map.insert(request_id.clone(), el.value());
                                    rejection_reasons.set(map);
                                })
                            };

                            let on_approve = {
                                let token = state.token.clone();
                                let reload = reload.clone();
                                let error = error.clone();
                                let success = success.clone();
                                let request_id = request_id.clone();
                                let username = req.username.clone();
                                Callback::from(move |_: MouseEvent| {
                                    if let Some(token) = token.clone() {
                                        let reload = reload.clone();
                                        let error = error.clone();
                                        let success = success.clone();
                                        let request_id = request_id.clone();
                                        let username = username.clone();
                                        spawn_local(async move {
                                            match admin_ops::approve_deletion_request(&token, &request_id).await {
                                                Ok(_) => {
                                                    success.set(format!("Approved deletion request for '{}'.", username));
                                                    error.set(String::new());
                                                    reload.emit(());
                                                }
                                                Err(e) => error.set(format!("Failed to approve deletion request: {}", e)),
                                            }
                                        });
                                    }
                                })
                            };

                            let on_reject = {
                                let token = state.token.clone();
                                let rejection_reasons = rejection_reasons.clone();
                                let reload = reload.clone();
                                let error = error.clone();
                                let success = success.clone();
                                let request_id = request_id.clone();
                                let username = req.username.clone();
                                Callback::from(move |_: MouseEvent| {
                                    if let Some(token) = token.clone() {
                                        let reason = rejection_reasons
                                            .get(&request_id)
                                            .and_then(|s| normalize_rejection_reason(s));
                                        let req_body = RejectDeletionRequest { reason };
                                        let reload = reload.clone();
                                        let error = error.clone();
                                        let success = success.clone();
                                        let request_id = request_id.clone();
                                        let username = username.clone();
                                        spawn_local(async move {
                                            match admin_ops::reject_deletion_request(&token, &request_id, &req_body).await {
                                                Ok(_) => {
                                                    success.set(format!("Rejected deletion request for '{}'.", username));
                                                    error.set(String::new());
                                                    reload.emit(());
                                                }
                                                Err(e) => error.set(format!("Failed to reject deletion request: {}", e)),
                                            }
                                        });
                                    }
                                })
                            };

                            html! {
                                <tr key={req.id.clone()}>
                                    <td>{ &req.username }</td>
                                    <td>{ &req.email }</td>
                                    <td>{ req.reason.clone().unwrap_or_default() }</td>
                                    <td>{ &req.requested_at[..19] }</td>
                                    <td>
                                        <input class="inline-input" type="text" value={reject_reason} oninput={on_reason_input} placeholder="Optional rejection reason" />
                                    </td>
                                    <td class="action-cell">
                                        <button class="btn-sm btn-success" onclick={on_approve}>{ "Approve" }</button>
                                        <button class="btn-sm btn-danger" onclick={on_reject}>{ "Reject" }</button>
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
    use super::normalize_rejection_reason;

    #[test]
    fn empty_rejection_reason_becomes_none() {
        assert_eq!(normalize_rejection_reason(""), None);
        assert_eq!(normalize_rejection_reason("   "), None);
    }

    #[test]
    fn non_empty_rejection_reason_is_trimmed() {
        assert_eq!(
            normalize_rejection_reason("  duplicate request  "),
            Some("duplicate request".to_string())
        );
    }
}
