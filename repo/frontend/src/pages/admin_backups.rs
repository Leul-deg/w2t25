use wasm_bindgen_futures::spawn_local;
use web_sys::HtmlInputElement;
use yew::prelude::*;

use crate::api::admin_ops::{self, BackupMeta, CreateBackupRequest, RestoreBackupResponse};
use crate::state::AppStateContext;

#[function_component(AdminBackupsPage)]
pub fn admin_backups_page() -> Html {
    let state = use_context::<AppStateContext>().expect("AppState context");
    let backups = use_state(Vec::<BackupMeta>::new);
    let restore_preview = use_state(|| None::<RestoreBackupResponse>);
    let notes = use_state(String::new);
    let loading = use_state(|| true);
    let error = use_state(String::new);
    let success = use_state(String::new);

    let reload = {
        let backups = backups.clone();
        let loading = loading.clone();
        let error = error.clone();
        let token = state.token.clone();
        Callback::from(move |_: ()| {
            let backups = backups.clone();
            let loading = loading.clone();
            let error = error.clone();
            let token = token.clone();
            loading.set(true);
            if let Some(token) = token {
                spawn_local(async move {
                    match admin_ops::list_backups(&token).await {
                        Ok(rows) => {
                            backups.set(rows);
                            error.set(String::new());
                        }
                        Err(e) => error.set(format!("Failed to load backups: {}", e)),
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

    let on_create = {
        let token = state.token.clone();
        let notes = notes.clone();
        let error = error.clone();
        let success = success.clone();
        let reload = reload.clone();
        Callback::from(move |_: MouseEvent| {
            if let Some(token) = token.clone() {
                let req = CreateBackupRequest {
                    notes: if notes.is_empty() { None } else { Some((*notes).clone()) },
                };
                let error = error.clone();
                let success = success.clone();
                let reload = reload.clone();
                spawn_local(async move {
                    match admin_ops::create_backup(&token, &req).await {
                        Ok(resp) => {
                            success.set(format!("Backup created: {}", resp.filename));
                            error.set(String::new());
                            reload.emit(());
                        }
                        Err(e) => error.set(format!("Create backup failed: {}", e)),
                    }
                });
            }
        })
    };

    html! {
        <div class="page-container">
            <h2>{ "Backups & Restore" }</h2>
            if !error.is_empty() { <div class="error-banner">{ (*error).clone() }</div> }
            if !success.is_empty() { <div class="success-banner">{ (*success).clone() }</div> }

            <div class="card">
                <h3>{ "Create Encrypted Backup" }</h3>
                <div class="form-row">
                    <label>{ "Notes" }</label>
                    <input type="text" value={(*notes).clone()} oninput={{
                        let notes = notes.clone();
                        Callback::from(move |e: InputEvent| {
                            let el: HtmlInputElement = e.target_unchecked_into();
                            notes.set(el.value());
                        })
                    }} placeholder="Optional notes" />
                </div>
                <div class="prefs-actions">
                    <button class="btn-primary" onclick={on_create}>{ "Create Backup" }</button>
                </div>
            </div>

            <div class="card">
                <h3>{ "Available Backups" }</h3>
                if *loading {
                    <p>{ "Loading..." }</p>
                } else {
                    <table class="data-table">
                        <thead>
                            <tr>
                                <th>{ "Filename" }</th>
                                <th>{ "Status" }</th>
                                <th>{ "Size" }</th>
                                <th>{ "Created" }</th>
                                <th>{ "Action" }</th>
                            </tr>
                        </thead>
                        <tbody>
                            { for backups.iter().map(|backup| {
                                let token = state.token.clone();
                                let restore_preview = restore_preview.clone();
                                let error = error.clone();
                                let backup_id = backup.id.clone();
                                let on_restore = Callback::from(move |_: MouseEvent| {
                                    if let Some(token) = token.clone() {
                                        let restore_preview = restore_preview.clone();
                                        let error = error.clone();
                                        let backup_id = backup_id.clone();
                                        spawn_local(async move {
                                            match admin_ops::restore_backup(&token, &backup_id).await {
                                                Ok(prep) => {
                                                    restore_preview.set(Some(prep));
                                                    error.set(String::new());
                                                }
                                                Err(e) => error.set(format!("Prepare restore failed: {}", e)),
                                            }
                                        });
                                    }
                                });
                                html!{
                                    <tr key={backup.id.clone()}>
                                        <td class="mono">{ &backup.filename }</td>
                                        <td>{ &backup.status }</td>
                                        <td>{ backup.size_bytes.unwrap_or(0) }</td>
                                        <td>{ &backup.created_at[..19] }</td>
                                        <td><button class="btn-sm btn-primary" onclick={on_restore}>{ "Prepare Restore" }</button></td>
                                    </tr>
                                }
                            }) }
                        </tbody>
                    </table>
                }
            </div>

            if let Some(prep) = &*restore_preview {
                <div class="card">
                    <h3>{ "Restore Instructions" }</h3>
                    <p>{ &prep.warning }</p>
                    <p class="mono">{ &prep.restore_path }</p>
                    <p class="mono">{ &prep.psql_command }</p>
                </div>
            }
        </div>
    }
}
