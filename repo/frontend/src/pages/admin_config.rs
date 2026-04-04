/// Configuration center page.
///
/// Shows all editable config values, allows updating them with an optional
/// reason.  Displays the change history.  Also shows campaign toggles.

use wasm_bindgen_futures::spawn_local;
use web_sys::HtmlInputElement;
use yew::prelude::*;

use crate::api::store::{
    self, CampaignToggle, ConfigHistoryEntry, ConfigValue, UpdateCampaignRequest,
    UpdateConfigRequest,
};
use crate::state::AppStateContext;

#[derive(Clone, PartialEq)]
enum ConfigTab {
    Values,
    Campaigns,
    History,
}

#[function_component(AdminConfigPage)]
pub fn admin_config_page() -> Html {
    let state = use_context::<AppStateContext>().expect("AppState context");
    let config_values = use_state(Vec::<ConfigValue>::new);
    let history = use_state(Vec::<ConfigHistoryEntry>::new);
    let campaigns = use_state(Vec::<CampaignToggle>::new);
    let loading = use_state(|| true);
    let error = use_state(String::new);
    let success = use_state(String::new);
    let active_tab = use_state(|| ConfigTab::Values);
    // Editing state: key → (new_value, reason)
    let edits: UseStateHandle<std::collections::HashMap<String, (String, String)>> =
        use_state(std::collections::HashMap::new);

    let reload = {
        let config_values = config_values.clone();
        let history = history.clone();
        let campaigns = campaigns.clone();
        let loading = loading.clone();
        let error = error.clone();
        let token = state.token.clone();
        Callback::from(move |_: ()| {
            let config_values = config_values.clone();
            let history = history.clone();
            let campaigns = campaigns.clone();
            let loading = loading.clone();
            let error = error.clone();
            loading.set(true);
            if let Some(token) = token.clone() {
                let t1 = token.clone();
                let t2 = token.clone();
                let t3 = token.clone();
                spawn_local(async move {
                    let cv = store::admin_get_config(&t1).await;
                    let hi = store::admin_get_config_history(&t2).await;
                    let ca = store::admin_get_campaigns(&t3).await;
                    match (cv, hi, ca) {
                        (Ok(cv), Ok(hi), Ok(ca)) => {
                            config_values.set(cv);
                            history.set(hi);
                            campaigns.set(ca);
                            loading.set(false);
                        }
                        _ => {
                            error.set("Failed to load configuration.".to_string());
                            loading.set(false);
                        }
                    }
                });
            }
        })
    };

    {
        let reload = reload.clone();
        use_effect_with((), move |_| { reload.emit(()); });
    }

    if *loading {
        return html! { <div class="card"><p>{ "Loading configuration\u{2026}" }</p></div> };
    }

    let tab_values = {
        let active_tab = active_tab.clone();
        Callback::from(move |_: MouseEvent| { active_tab.set(ConfigTab::Values); })
    };
    let tab_campaigns = {
        let active_tab = active_tab.clone();
        Callback::from(move |_: MouseEvent| { active_tab.set(ConfigTab::Campaigns); })
    };
    let tab_history = {
        let active_tab = active_tab.clone();
        Callback::from(move |_: MouseEvent| { active_tab.set(ConfigTab::History); })
    };

    html! {
        <div class="page-container">
            <h2>{ "Configuration Center" }</h2>

            if !error.is_empty() {
                <div class="error-banner">{ error.as_str() }</div>
            }
            if !success.is_empty() {
                <div class="success-banner">{ success.as_str() }</div>
            }

            <div class="tab-bar">
                <button
                    class={if *active_tab == ConfigTab::Values { "tab active" } else { "tab" }}
                    onclick={tab_values}
                >{ "Config Values" }</button>
                <button
                    class={if *active_tab == ConfigTab::Campaigns { "tab active" } else { "tab" }}
                    onclick={tab_campaigns}
                >{ "Campaigns" }</button>
                <button
                    class={if *active_tab == ConfigTab::History { "tab active" } else { "tab" }}
                    onclick={tab_history}
                >{ "Change History" }</button>
            </div>

            // ── Config Values tab ─────────────────────────────────────────
            if *active_tab == ConfigTab::Values {
                <table class="data-table">
                    <thead>
                        <tr>
                            <th>{ "Key" }</th>
                            <th>{ "Current Value" }</th>
                            <th>{ "Type" }</th>
                            <th>{ "Description" }</th>
                            <th>{ "New Value" }</th>
                            <th>{ "Reason" }</th>
                            <th>{ "Save" }</th>
                        </tr>
                    </thead>
                    <tbody>
                        { config_values.iter().map(|cv| {
                            let key = cv.key.clone();
                            let edits_clone = edits.clone();
                            let (edit_val, edit_reason) = edits
                                .get(&key)
                                .cloned()
                                .unwrap_or_else(|| (cv.value.clone().unwrap_or_default(), String::new()));

                            let on_val_input = {
                                let key = key.clone();
                                let edits = edits.clone();
                                let reason = edit_reason.clone();
                                Callback::from(move |e: InputEvent| {
                                    let el: HtmlInputElement = e.target_unchecked_into();
                                    let mut map = (*edits).clone();
                                    map.insert(key.clone(), (el.value(), reason.clone()));
                                    edits.set(map);
                                })
                            };

                            let on_reason_input = {
                                let key = key.clone();
                                let edits = edits.clone();
                                let val = edit_val.clone();
                                Callback::from(move |e: InputEvent| {
                                    let el: HtmlInputElement = e.target_unchecked_into();
                                    let mut map = (*edits).clone();
                                    map.insert(key.clone(), (val.clone(), el.value()));
                                    edits.set(map);
                                })
                            };

                            let on_save = {
                                let key = key.clone();
                                let edits = edits_clone.clone();
                                let token = state.token.clone().unwrap_or_default();
                                let error = error.clone();
                                let success = success.clone();
                                let reload = reload.clone();
                                Callback::from(move |_: MouseEvent| {
                                    let (val, reason) = edits
                                        .get(&key)
                                        .cloned()
                                        .unwrap_or_default();
                                    let req = UpdateConfigRequest {
                                        value: val,
                                        reason: if reason.is_empty() { None } else { Some(reason) },
                                    };
                                    let key = key.clone();
                                    let token = token.clone();
                                    let error = error.clone();
                                    let success = success.clone();
                                    let reload = reload.clone();
                                    spawn_local(async move {
                                        match store::admin_update_config(&token, &key, &req).await {
                                            Ok(_) => {
                                                success.set(format!("'{}' updated.", key));
                                                error.set(String::new());
                                                reload.emit(());
                                            }
                                            Err(e) => {
                                                error.set(format!("Update failed: {}", e));
                                                success.set(String::new());
                                            }
                                        }
                                    });
                                })
                            };

                            html! {
                                <tr key={cv.id.to_string()}>
                                    <td class="mono">{ &cv.key }</td>
                                    <td class="mono">{ cv.value.as_deref().unwrap_or("—") }</td>
                                    <td>{ &cv.value_type }</td>
                                    <td>{ cv.description.as_deref().unwrap_or("") }</td>
                                    <td>
                                        <input
                                            type="text"
                                            class="inline-input"
                                            value={edit_val.clone()}
                                            oninput={on_val_input}
                                        />
                                    </td>
                                    <td>
                                        <input
                                            type="text"
                                            class="inline-input"
                                            value={edit_reason.clone()}
                                            oninput={on_reason_input}
                                            placeholder="Optional reason"
                                        />
                                    </td>
                                    <td>
                                        <button class="btn-sm btn-primary" onclick={on_save}>
                                            { "Save" }
                                        </button>
                                    </td>
                                </tr>
                            }
                        }).collect::<Html>() }
                    </tbody>
                </table>
            }

            // ── Campaigns tab ─────────────────────────────────────────────
            if *active_tab == ConfigTab::Campaigns {
                <table class="data-table">
                    <thead>
                        <tr>
                            <th>{ "Campaign" }</th>
                            <th>{ "Description" }</th>
                            <th>{ "Enabled" }</th>
                            <th>{ "Toggle" }</th>
                        </tr>
                    </thead>
                    <tbody>
                        { campaigns.iter().map(|c| {
                            let name = c.name.clone();
                            let currently_enabled = c.enabled;
                            let token = state.token.clone().unwrap_or_default();
                            let error = error.clone();
                            let success = success.clone();
                            let reload = reload.clone();

                            let on_toggle = Callback::from(move |_: MouseEvent| {
                                let new_enabled = !currently_enabled;
                                let req = UpdateCampaignRequest { enabled: new_enabled };
                                let name = name.clone();
                                let token = token.clone();
                                let error = error.clone();
                                let success = success.clone();
                                let reload = reload.clone();
                                spawn_local(async move {
                                    match store::admin_update_campaign(&token, &name, &req).await {
                                        Ok(_) => {
                                            success.set(format!(
                                                "Campaign '{}' {}.",
                                                name,
                                                if new_enabled { "enabled" } else { "disabled" }
                                            ));
                                            error.set(String::new());
                                            reload.emit(());
                                        }
                                        Err(e) => {
                                            error.set(format!("Toggle failed: {}", e));
                                        }
                                    }
                                });
                            });

                            html! {
                                <tr key={c.id.to_string()}>
                                    <td class="mono">{ &c.name }</td>
                                    <td>{ c.description.as_deref().unwrap_or("") }</td>
                                    <td>
                                        <span class={if c.enabled { "badge badge-success" } else { "badge badge-danger" }}>
                                            { if c.enabled { "ON" } else { "OFF" } }
                                        </span>
                                    </td>
                                    <td>
                                        <button
                                            class={if c.enabled { "btn-sm btn-danger" } else { "btn-sm btn-success" }}
                                            onclick={on_toggle}
                                        >
                                            { if c.enabled { "Disable" } else { "Enable" } }
                                        </button>
                                    </td>
                                </tr>
                            }
                        }).collect::<Html>() }
                    </tbody>
                </table>
            }

            // ── History tab ───────────────────────────────────────────────
            if *active_tab == ConfigTab::History {
                <table class="data-table">
                    <thead>
                        <tr>
                            <th>{ "Key" }</th>
                            <th>{ "Old Value" }</th>
                            <th>{ "New Value" }</th>
                            <th>{ "Changed By" }</th>
                            <th>{ "When" }</th>
                            <th>{ "Reason" }</th>
                        </tr>
                    </thead>
                    <tbody>
                        { history.iter().map(|h| html! {
                            <tr key={h.id.to_string()}>
                                <td class="mono">{ &h.config_key }</td>
                                <td class="mono">{ h.old_value.as_deref().unwrap_or("—") }</td>
                                <td class="mono">{ h.new_value.as_deref().unwrap_or("—") }</td>
                                <td>{ h.changed_by_username.as_deref().unwrap_or("system") }</td>
                                <td>{ &h.changed_at[..19] }</td>
                                <td>{ h.reason.as_deref().unwrap_or("") }</td>
                            </tr>
                        }).collect::<Html>() }
                    </tbody>
                </table>
            }
        </div>
    }
}
