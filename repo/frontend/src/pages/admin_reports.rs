use wasm_bindgen_futures::spawn_local;
use web_sys::{HtmlInputElement, HtmlSelectElement};
use yew::prelude::*;

use crate::api::admin_ops::{self, CreateReportRequest, ReportJob};
use crate::state::AppStateContext;

#[function_component(AdminReportsPage)]
pub fn admin_reports_page() -> Html {
    let state = use_context::<AppStateContext>().expect("AppState context");
    let reports = use_state(Vec::<ReportJob>::new);
    let loading = use_state(|| true);
    let error = use_state(String::new);
    let success = use_state(String::new);
    let report_type = use_state(|| "orders".to_string());
    let start_date = use_state(|| "2026-03-01".to_string());
    let end_date = use_state(|| "2026-03-31".to_string());
    let pii_masked = use_state(|| true);

    let reload = {
        let reports = reports.clone();
        let loading = loading.clone();
        let error = error.clone();
        let token = state.token.clone();
        Callback::from(move |_: ()| {
            let reports = reports.clone();
            let loading = loading.clone();
            let error = error.clone();
            let token = token.clone();
            loading.set(true);
            if let Some(token) = token {
                spawn_local(async move {
                    match admin_ops::list_reports(&token).await {
                        Ok(rows) => {
                            reports.set(rows);
                            error.set(String::new());
                        }
                        Err(e) => error.set(format!("Failed to load reports: {}", e)),
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
        let report_type = report_type.clone();
        let start_date = start_date.clone();
        let end_date = end_date.clone();
        let pii_masked = pii_masked.clone();
        let error = error.clone();
        let success = success.clone();
        let reload = reload.clone();
        Callback::from(move |_: MouseEvent| {
            if let Some(token) = token.clone() {
                let req = CreateReportRequest {
                    report_type: (*report_type).clone(),
                    start_date: (*start_date).clone(),
                    end_date: (*end_date).clone(),
                    pii_masked: *pii_masked,
                };
                let error = error.clone();
                let success = success.clone();
                let reload = reload.clone();
                spawn_local(async move {
                    match admin_ops::create_report(&token, &req).await {
                        Ok(resp) => {
                            success.set(format!("Created {} report with {} rows.", resp.report_type, resp.row_count));
                            error.set(String::new());
                            reload.emit(());
                        }
                        Err(e) => error.set(format!("Create report failed: {}", e)),
                    }
                });
            }
        })
    };

    html! {
        <div class="page-container">
            <h2>{ "Reports & Exports" }</h2>
            if !error.is_empty() { <div class="error-banner">{ (*error).clone() }</div> }
            if !success.is_empty() { <div class="success-banner">{ (*success).clone() }</div> }

            <div class="card">
                <h3>{ "Generate Report" }</h3>
                <div class="form-row">
                    <label>{ "Report Type" }</label>
                    <select onchange={{
                        let report_type = report_type.clone();
                        Callback::from(move |e: Event| {
                            let el: HtmlSelectElement = e.target_unchecked_into();
                            report_type.set(el.value());
                        })
                    }}>
                        <option value="orders">{ "Orders" }</option>
                        <option value="checkins">{ "Check-Ins" }</option>
                        <option value="approvals">{ "Approvals / Denials" }</option>
                        <option value="kpi">{ "KPI" }</option>
                        <option value="operational">{ "Operational" }</option>
                    </select>
                </div>
                <div class="form-row">
                    <label>{ "Start Date" }</label>
                    <input type="date" value={(*start_date).clone()} oninput={{
                        let start_date = start_date.clone();
                        Callback::from(move |e: InputEvent| {
                            let el: HtmlInputElement = e.target_unchecked_into();
                            start_date.set(el.value());
                        })
                    }} />
                </div>
                <div class="form-row">
                    <label>{ "End Date" }</label>
                    <input type="date" value={(*end_date).clone()} oninput={{
                        let end_date = end_date.clone();
                        Callback::from(move |e: InputEvent| {
                            let el: HtmlInputElement = e.target_unchecked_into();
                            end_date.set(el.value());
                        })
                    }} />
                </div>
                <div class="prefs-toggle-row">
                    <span class="toggle-label">{ "Mask PII" }</span>
                    <button class={if *pii_masked { "toggle-btn toggle-on" } else { "toggle-btn toggle-off" }} onclick={{
                        let pii_masked = pii_masked.clone();
                        Callback::from(move |_: MouseEvent| pii_masked.set(!*pii_masked))
                    }}>
                        { if *pii_masked { "On" } else { "Off" } }
                    </button>
                </div>
                <div class="prefs-actions">
                    <button class="btn-primary" onclick={on_create}>{ "Generate Report" }</button>
                </div>
            </div>

            <div class="card">
                <h3>{ "Recent Report Jobs" }</h3>
                if *loading {
                    <p>{ "Loading..." }</p>
                } else {
                    <table class="data-table">
                        <thead>
                            <tr>
                                <th>{ "Type" }</th>
                                <th>{ "Status" }</th>
                                <th>{ "Masked" }</th>
                                <th>{ "Rows" }</th>
                                <th>{ "Created" }</th>
                                <th>{ "Output" }</th>
                            </tr>
                        </thead>
                        <tbody>
                            { for reports.iter().map(|job| html!{
                                <tr key={job.id.clone()}>
                                    <td>{ &job.report_type }</td>
                                    <td>{ &job.status }</td>
                                    <td>{ if job.pii_masked { "Yes" } else { "No" } }</td>
                                    <td>{ job.row_count.unwrap_or(0) }</td>
                                    <td>{ &job.created_at[..19] }</td>
                                    <td class="mono">{ job.output_path.clone().unwrap_or_else(|| "—".into()) }</td>
                                </tr>
                            }) }
                        </tbody>
                    </table>
                }
            </div>
        </div>
    }
}
