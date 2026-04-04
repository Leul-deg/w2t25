use wasm_bindgen_futures::spawn_local;
use yew::prelude::*;

use crate::api::admin_ops::{self, AccessLogRow, AuditLogRow, ErrorLogRow};
use crate::state::AppStateContext;

#[derive(Clone, PartialEq)]
enum LogTab {
    Audit,
    Access,
    Errors,
}

#[function_component(AdminLogsPage)]
pub fn admin_logs_page() -> Html {
    let state = use_context::<AppStateContext>().expect("AppState context");
    let tab = use_state(|| LogTab::Audit);
    let audit_rows = use_state(Vec::<AuditLogRow>::new);
    let access_rows = use_state(Vec::<AccessLogRow>::new);
    let error_rows = use_state(Vec::<ErrorLogRow>::new);
    let loading = use_state(|| true);
    let error = use_state(String::new);
    let success = use_state(String::new);

    let reload = {
        let tab = tab.clone();
        let audit_rows = audit_rows.clone();
        let access_rows = access_rows.clone();
        let error_rows = error_rows.clone();
        let loading = loading.clone();
        let error = error.clone();
        let token = state.token.clone();
        Callback::from(move |_: ()| {
            let tab = (*tab).clone();
            let audit_rows = audit_rows.clone();
            let access_rows = access_rows.clone();
            let error_rows = error_rows.clone();
            let loading = loading.clone();
            let error = error.clone();
            let token = token.clone();
            loading.set(true);
            if let Some(token) = token {
                spawn_local(async move {
                    let result = match tab {
                        LogTab::Audit => admin_ops::audit_logs(&token).await.map(|r| {
                            audit_rows.set(r.rows);
                        }),
                        LogTab::Access => admin_ops::access_logs(&token).await.map(|r| {
                            access_rows.set(r.rows);
                        }),
                        LogTab::Errors => admin_ops::error_logs(&token).await.map(|r| {
                            error_rows.set(r.rows);
                        }),
                    };
                    match result {
                        Ok(_) => error.set(String::new()),
                        Err(e) => error.set(format!("Failed to load logs: {}", e)),
                    }
                    loading.set(false);
                });
            }
        })
    };

    {
        let reload = reload.clone();
        let tab_dep = (*tab).clone();
        use_effect_with(tab_dep, move |_| reload.emit(()));
    }

    let on_prune = {
        let token = state.token.clone();
        let error = error.clone();
        let success = success.clone();
        let reload = reload.clone();
        Callback::from(move |_: MouseEvent| {
            if let Some(token) = token.clone() {
                let error = error.clone();
                let success = success.clone();
                let reload = reload.clone();
                spawn_local(async move {
                    match admin_ops::prune_logs(&token).await {
                        Ok(_) => {
                            success.set("Log pruning completed.".to_string());
                            error.set(String::new());
                            reload.emit(());
                        }
                        Err(e) => error.set(format!("Prune failed: {}", e)),
                    }
                });
            }
        })
    };

    html! {
        <div class="page-container">
            <h2>{ "Logs & Retention" }</h2>
            if !error.is_empty() { <div class="error-banner">{ (*error).clone() }</div> }
            if !success.is_empty() { <div class="success-banner">{ (*success).clone() }</div> }

            <div class="tab-bar">
                <button class={if *tab == LogTab::Audit { "tab active" } else { "tab" }} onclick={{
                    let tab = tab.clone();
                    Callback::from(move |_: MouseEvent| tab.set(LogTab::Audit))
                }}>{ "Audit" }</button>
                <button class={if *tab == LogTab::Access { "tab active" } else { "tab" }} onclick={{
                    let tab = tab.clone();
                    Callback::from(move |_: MouseEvent| tab.set(LogTab::Access))
                }}>{ "Access" }</button>
                <button class={if *tab == LogTab::Errors { "tab active" } else { "tab" }} onclick={{
                    let tab = tab.clone();
                    Callback::from(move |_: MouseEvent| tab.set(LogTab::Errors))
                }}>{ "Errors" }</button>
                <button class="btn-sm btn-danger" onclick={on_prune}>{ "Prune 180d+" }</button>
            </div>

            <div class="card">
                if *loading {
                    <p>{ "Loading..." }</p>
                } else if *tab == LogTab::Audit {
                    <table class="data-table">
                        <thead><tr><th>{ "When" }</th><th>{ "Actor" }</th><th>{ "Action" }</th><th>{ "Entity" }</th></tr></thead>
                        <tbody>{ for audit_rows.iter().map(|row| html!{
                            <tr key={row.id.clone()}>
                                <td>{ &row.created_at[..19] }</td>
                                <td>{ row.actor_username.clone().unwrap_or_else(|| "system".into()) }</td>
                                <td>{ &row.action }</td>
                                <td>{ format!("{} {}", row.entity_type, row.entity_id.clone().unwrap_or_default()) }</td>
                            </tr>
                        }) }</tbody>
                    </table>
                } else if *tab == LogTab::Access {
                    <table class="data-table">
                        <thead><tr><th>{ "When" }</th><th>{ "User" }</th><th>{ "Action" }</th><th>{ "Success" }</th><th>{ "Details" }</th></tr></thead>
                        <tbody>{ for access_rows.iter().map(|row| html!{
                            <tr key={row.id.clone()}>
                                <td>{ &row.created_at[..19] }</td>
                                <td>{ row.username.clone().unwrap_or_else(|| "anonymous".into()) }</td>
                                <td>{ &row.action }</td>
                                <td>{ if row.success { "Yes" } else { "No" } }</td>
                                <td>{ row.details.clone().unwrap_or_default() }</td>
                            </tr>
                        }) }</tbody>
                    </table>
                } else {
                    <table class="data-table">
                        <thead><tr><th>{ "When" }</th><th>{ "Level" }</th><th>{ "Message" }</th><th>{ "Path" }</th></tr></thead>
                        <tbody>{ for error_rows.iter().map(|row| html!{
                            <tr key={row.id.clone()}>
                                <td>{ &row.created_at[..19] }</td>
                                <td>{ &row.level }</td>
                                <td>{ &row.message }</td>
                                <td>{ row.request_path.clone().unwrap_or_default() }</td>
                            </tr>
                        }) }</tbody>
                    </table>
                }
            </div>
        </div>
    }
}
