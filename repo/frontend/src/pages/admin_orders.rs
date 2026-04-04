/// Admin order management page.
///
/// Includes a near-real-time dashboard, low-stock alerts, order detail, and
/// status transitions for operations staff.

use gloo_timers::callback::Interval;
use wasm_bindgen_futures::spawn_local;
use yew::prelude::*;

use crate::api::store::{self, AdminOrder, OrderDetail, OrdersDashboard, UpdateOrderStatusRequest};
use crate::state::AppStateContext;

#[function_component(AdminOrdersPage)]
pub fn admin_orders_page() -> Html {
    let state = use_context::<AppStateContext>().expect("AppState context");
    let orders = use_state(Vec::<AdminOrder>::new);
    let dashboard = use_state(|| None::<OrdersDashboard>);
    let selected_order = use_state(|| None::<OrderDetail>);
    let loading = use_state(|| true);
    let error = use_state(String::new);
    let success = use_state(String::new);

    let reload = {
        let orders = orders.clone();
        let dashboard = dashboard.clone();
        let loading = loading.clone();
        let error = error.clone();
        let token = state.token.clone();
        Callback::from(move |_: ()| {
            let orders = orders.clone();
            let dashboard = dashboard.clone();
            let loading = loading.clone();
            let error = error.clone();
            let token = token.clone();
            loading.set(true);
            if let Some(token) = token {
                spawn_local(async move {
                    match (
                        store::admin_get_orders(&token).await,
                        store::admin_get_orders_dashboard(&token).await,
                    ) {
                        (Ok(list), Ok(dash)) => {
                            orders.set(list);
                            dashboard.set(Some(dash));
                            loading.set(false);
                        }
                        (Err(e), _) | (_, Err(e)) => {
                            error.set(format!("Load failed: {}", e));
                            loading.set(false);
                        }
                    }
                });
            }
        })
    };

    {
        let reload = reload.clone();
        use_effect_with((), move |_| {
            reload.emit(());
        });
    }

    {
        let reload = reload.clone();
        use_effect_with((), move |_| {
            let interval = Interval::new(30_000, move || reload.emit(()));
            move || drop(interval)
        });
    }

    let make_status_handler = {
        let state = state.clone();
        let error = error.clone();
        let success = success.clone();
        let reload = reload.clone();
        move |order_id: uuid::Uuid, new_status: &'static str| {
            let token = state.token.clone().unwrap_or_default();
            let error = error.clone();
            let success = success.clone();
            let reload = reload.clone();
            Callback::from(move |e: MouseEvent| {
                e.stop_propagation();
                let token = token.clone();
                let error = error.clone();
                let success = success.clone();
                let reload = reload.clone();
                spawn_local(async move {
                    let req = UpdateOrderStatusRequest { status: new_status.to_string() };
                    match store::admin_update_order_status(&token, &order_id, &req).await {
                        Ok(_) => {
                            success.set(format!("Order updated to '{}'.", new_status));
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
        }
    };

    if *loading {
        return html! { <div class="card"><p>{ "Loading orders\u{2026}" }</p></div> };
    }

    html! {
        <div class="page-container">
            <h2>{ "Order Management" }</h2>

            if !error.is_empty() {
                <div class="error-banner">{ error.as_str() }</div>
            }
            if !success.is_empty() {
                <div class="success-banner">{ success.as_str() }</div>
            }

            if let Some(dash) = &*dashboard {
                <div class="dashboard-grid">
                    <div class="dashboard-card"><h3>{ "Pending" }</h3><p>{ dash.pending_orders }</p></div>
                    <div class="dashboard-card"><h3>{ "Confirmed" }</h3><p>{ dash.confirmed_orders }</p></div>
                    <div class="dashboard-card"><h3>{ "Fulfilled" }</h3><p>{ dash.fulfilled_orders }</p></div>
                    <div class="dashboard-card"><h3>{ "Cancelled" }</h3><p>{ dash.cancelled_orders }</p></div>
                    <div class="dashboard-card"><h3>{ "Pending > 30 Min" }</h3><p>{ dash.pending_over_30_min }</p></div>
                    <div class="dashboard-card"><h3>{ "Low Stock Alerts" }</h3><p>{ dash.low_stock_products.len() }</p></div>
                </div>
            }

            <p class="hint">{ format!("{} total orders", orders.len()) }</p>

            if orders.is_empty() {
                <div class="card"><p>{ "No orders found." }</p></div>
            } else {
                <table class="data-table">
                    <thead>
                        <tr>
                            <th>{ "Order ID" }</th>
                            <th>{ "Customer" }</th>
                            <th>{ "Status" }</th>
                            <th>{ "Total" }</th>
                            <th>{ "Items" }</th>
                            <th>{ "Points" }</th>
                            <th>{ "Placed" }</th>
                            <th>{ "Actions" }</th>
                        </tr>
                    </thead>
                    <tbody>
                        { orders.iter().map(|o| {
                            let oid = o.id;
                            let token = state.token.clone();
                            let selected_order = selected_order.clone();
                            let error = error.clone();
                            let on_open = Callback::from(move |_: MouseEvent| {
                                if let Some(token) = token.clone() {
                                    let selected_order = selected_order.clone();
                                    let error = error.clone();
                                    spawn_local(async move {
                                        match store::admin_get_order_detail(&token, &oid).await {
                                            Ok(detail) => {
                                                selected_order.set(Some(detail));
                                                error.set(String::new());
                                            }
                                            Err(e) => error.set(format!("Detail load failed: {}", e)),
                                        }
                                    });
                                }
                            });

                            let status_class = match o.status.as_str() {
                                "pending"   => "badge-warning",
                                "confirmed" => "badge-info",
                                "fulfilled" => "badge-success",
                                "cancelled" | "refunded" => "badge-danger",
                                _ => "badge-default",
                            };

                            let can_confirm  = o.status == "pending";
                            let can_fulfill  = o.status == "confirmed";
                            let can_cancel   = matches!(o.status.as_str(), "pending" | "confirmed");

                            let on_confirm = make_status_handler(oid, "confirmed");
                            let on_fulfill = make_status_handler(oid, "fulfilled");
                            let on_cancel  = make_status_handler(oid, "cancelled");

                            html! {
                                <tr key={o.id.to_string()} onclick={on_open} class="clickable-row">
                                    <td class="mono">{ &o.id.to_string()[..8] }{ "…" }</td>
                                    <td>{ &o.username }</td>
                                    <td>
                                        <span class={classes!("badge", status_class)}>
                                            { &o.status }
                                        </span>
                                    </td>
                                    <td>{ o.total_display() }</td>
                                    <td>{ o.item_count }</td>
                                    <td>{ o.points_earned }{ " pts" }</td>
                                    <td>{ &o.created_at[..10] }</td>
                                    <td class="action-cell">
                                        if can_confirm {
                                            <button class="btn-sm btn-primary" onclick={on_confirm}>
                                                { "Confirm" }
                                            </button>
                                        }
                                        if can_fulfill {
                                            <button class="btn-sm btn-success" onclick={on_fulfill}>
                                                { "Fulfill" }
                                            </button>
                                        }
                                        if can_cancel {
                                            <button class="btn-sm btn-danger" onclick={on_cancel}>
                                                { "Cancel" }
                                            </button>
                                        }
                                    </td>
                                </tr>
                            }
                        }).collect::<Html>() }
                    </tbody>
                </table>
            }

            if let Some(dash) = &*dashboard {
                if !dash.low_stock_products.is_empty() {
                    <div class="card">
                        <h3>{ "Low Stock Products" }</h3>
                        <table class="data-table">
                            <thead>
                                <tr>
                                    <th>{ "Product" }</th>
                                    <th>{ "Qty" }</th>
                                    <th>{ "Threshold" }</th>
                                </tr>
                            </thead>
                            <tbody>
                                { for dash.low_stock_products.iter().map(|product| html! {
                                    <tr key={product.product_id.to_string()}>
                                        <td>{ &product.product_name }</td>
                                        <td>{ product.quantity }</td>
                                        <td>{ product.low_stock_threshold }</td>
                                    </tr>
                                }) }
                            </tbody>
                        </table>
                    </div>
                }
            }

            if let Some(detail) = &*selected_order {
                <div class="card">
                    <h3>{ format!("Order {} detail", &detail.id.to_string()[..8]) }</h3>
                    <p>{ format!(
                        "Status: {} | Total: ${:.2} | Shipping: ${:.2} | Points: {}",
                        detail.status,
                        detail.total_cents as f64 / 100.0,
                        detail.shipping_fee_cents as f64 / 100.0,
                        detail.points_earned
                    ) }</p>
                    <table class="data-table">
                        <thead>
                            <tr>
                                <th>{ "Product" }</th>
                                <th>{ "Qty" }</th>
                                <th>{ "Unit Price" }</th>
                                <th>{ "Subtotal" }</th>
                            </tr>
                        </thead>
                        <tbody>
                            { for detail.items.iter().map(|item| html! {
                                <tr key={item.id.to_string()}>
                                    <td>{ &item.product_name }</td>
                                    <td>{ item.quantity }</td>
                                    <td>{ format!("${:.2}", item.unit_price_cents as f64 / 100.0) }</td>
                                    <td>{ item.subtotal_display() }</td>
                                </tr>
                            }) }
                        </tbody>
                    </table>
                </div>
            }
        </div>
    }
}
