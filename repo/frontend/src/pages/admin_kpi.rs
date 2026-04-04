/// KPI dashboard page.
///
/// Displays daily sales, average order value, and repeat purchase rate.
/// All values are fetched from GET /admin/kpi.

use wasm_bindgen_futures::spawn_local;
use yew::prelude::*;

use crate::api::store::{self, KpiData};
use crate::state::AppStateContext;

#[function_component(AdminKpiPage)]
pub fn admin_kpi_page() -> Html {
    let state = use_context::<AppStateContext>().expect("AppState context");
    let kpi = use_state(|| None::<KpiData>);
    let loading = use_state(|| true);
    let error = use_state(String::new);

    {
        let kpi = kpi.clone();
        let loading = loading.clone();
        let error = error.clone();
        let token = state.token.clone();
        use_effect_with((), move |_| {
            if let Some(token) = token {
                spawn_local(async move {
                    match store::admin_get_kpi(&token).await {
                        Ok(data) => {
                            kpi.set(Some(data));
                            loading.set(false);
                        }
                        Err(e) => {
                            error.set(format!("Failed to load KPIs: {}", e));
                            loading.set(false);
                        }
                    }
                });
            } else {
                error.set("Not authenticated.".to_string());
                loading.set(false);
            }
        });
    }

    if *loading {
        return html! { <div class="card"><p>{ "Loading KPIs\u{2026}" }</p></div> };
    }

    html! {
        <div class="page-container">
            <h2>{ "KPI Dashboard" }</h2>

            if !error.is_empty() {
                <div class="error-banner">{ error.as_str() }</div>
            }

            if let Some(ref data) = *kpi {
                <div class="kpi-grid">
                    <div class="kpi-card">
                        <div class="kpi-label">{ "Daily Sales (Today)" }</div>
                        <div class="kpi-value">{ data.daily_sales_display() }</div>
                        <div class="kpi-sub">{ "Confirmed + Fulfilled orders, UTC today" }</div>
                    </div>

                    <div class="kpi-card">
                        <div class="kpi-label">{ "Average Order Value" }</div>
                        <div class="kpi-value">{ data.avg_order_display() }</div>
                        <div class="kpi-sub">{ "Confirmed + Fulfilled, last 30 days" }</div>
                    </div>

                    <div class="kpi-card">
                        <div class="kpi-label">{ "Repeat Purchase Rate" }</div>
                        <div class="kpi-value">{ data.repeat_rate_display() }</div>
                        <div class="kpi-sub">{ "Buyers with >1 order / all buyers, last 30 days" }</div>
                    </div>

                    <div class="kpi-card">
                        <div class="kpi-label">{ "Orders (Last 30d)" }</div>
                        <div class="kpi-value">{ data.orders_last_30d }</div>
                        <div class="kpi-sub">{ "Confirmed + Fulfilled" }</div>
                    </div>

                    <div class="kpi-card">
                        <div class="kpi-label">{ "Unique Buyers (Last 30d)" }</div>
                        <div class="kpi-value">{ data.buyers_last_30d }</div>
                        <div class="kpi-sub">{ "Distinct customers with qualifying orders" }</div>
                    </div>

                    <div class="kpi-card">
                        <div class="kpi-label">{ "Repeat Buyers (Last 30d)" }</div>
                        <div class="kpi-value">{ data.repeat_buyers_last_30d }</div>
                        <div class="kpi-sub">{ "Customers with more than one order" }</div>
                    </div>
                </div>

                <div class="card kpi-definitions">
                    <h3>{ "Metric Definitions" }</h3>
                    <dl>
                        <dt>{ "Daily Sales" }</dt>
                        <dd>{ "SUM(total_cents) for orders with status IN ('confirmed', 'fulfilled') created on the current UTC calendar day." }</dd>

                        <dt>{ "Average Order Value" }</dt>
                        <dd>{ "AVG(total_cents) for orders with status IN ('confirmed', 'fulfilled') created in the last 30 days. Returns $0.00 when there are no qualifying orders." }</dd>

                        <dt>{ "Repeat Purchase Rate" }</dt>
                        <dd>{ "(COUNT of user_ids with >1 qualifying order) / (COUNT of distinct user_ids with any qualifying order) × 100. Qualifying = confirmed or fulfilled, last 30 days. Returns 0% when there are no buyers." }</dd>
                    </dl>
                </div>
            }
        </div>
    }
}
