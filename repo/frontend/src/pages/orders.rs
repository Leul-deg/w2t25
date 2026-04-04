/// My Orders page — shows the authenticated user's order history.

use wasm_bindgen_futures::spawn_local;
use yew::prelude::*;

use crate::api::store::{self, OrderDetail, OrderSummary};
use crate::state::AppStateContext;

#[function_component(OrdersPage)]
pub fn orders_page() -> Html {
    let state = use_context::<AppStateContext>().expect("AppState context");
    let orders = use_state(Vec::<OrderSummary>::new);
    let selected_order = use_state(|| None::<OrderDetail>);
    let loading = use_state(|| true);
    let error = use_state(String::new);

    {
        let orders = orders.clone();
        let loading = loading.clone();
        let error = error.clone();
        let token = state.token.clone();
        use_effect_with((), move |_| {
            if let Some(token) = token {
                spawn_local(async move {
                    match store::get_my_orders(&token).await {
                        Ok(list) => {
                            orders.set(list);
                            loading.set(false);
                        }
                        Err(e) => {
                            error.set(format!("Failed to load orders: {}", e));
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
        return html! { <div class="card"><p>{ "Loading orders\u{2026}" }</p></div> };
    }

    html! {
        <div class="page-container">
            <h2>{ "My Orders" }</h2>

            if !error.is_empty() {
                <div class="error-banner">{ error.as_str() }</div>
            }

            if orders.is_empty() {
                <div class="card">
                    <p>{ "You have not placed any orders yet." }</p>
                </div>
            } else {
                <table class="data-table">
                    <thead>
                        <tr>
                            <th>{ "Order ID" }</th>
                            <th>{ "Status" }</th>
                            <th>{ "Total" }</th>
                            <th>{ "Shipping" }</th>
                            <th>{ "Points" }</th>
                            <th>{ "Placed" }</th>
                        </tr>
                    </thead>
                    <tbody>
                        { orders.iter().map(|o| {
                            let order_id = o.id;
                            let token = state.token.clone();
                            let selected_order = selected_order.clone();
                            let error = error.clone();
                            let on_open = Callback::from(move |_: MouseEvent| {
                                if let Some(token) = token.clone() {
                                    let selected_order = selected_order.clone();
                                    let error = error.clone();
                                    spawn_local(async move {
                                        match store::get_my_order_detail(&token, &order_id).await {
                                            Ok(detail) => {
                                                selected_order.set(Some(detail));
                                                error.set(String::new());
                                            }
                                            Err(e) => error.set(format!("Failed to load order detail: {}", e)),
                                        }
                                    });
                                }
                            });
                            let status_class = match o.status.as_str() {
                                "pending"   => "badge-warning",
                                "confirmed" => "badge-info",
                                "fulfilled" => "badge-success",
                                "cancelled" => "badge-danger",
                                _           => "badge-default",
                            };
                            html! {
                                <tr key={o.id.to_string()} onclick={on_open} class="clickable-row">
                                    <td class="mono">{ &o.id.to_string()[..8] }{ "…" }</td>
                                    <td>
                                        <span class={classes!("badge", status_class)}>
                                            { &o.status }
                                        </span>
                                    </td>
                                    <td>{ o.total_display() }</td>
                                    <td>{ o.shipping_display() }</td>
                                    <td>{ o.points_earned }{ " pts" }</td>
                                    <td>{ &o.created_at[..10] }</td>
                                </tr>
                            }
                        }).collect::<Html>() }
                    </tbody>
                </table>
            }

            if let Some(detail) = &*selected_order {
                <div class="card">
                    <h3>{ format!("Order {} details", &detail.id.to_string()[..8]) }</h3>
                    <p>{ format!("Status: {} | Shipping: ${:.2} | Points: {}", detail.status, detail.shipping_fee_cents as f64 / 100.0, detail.points_earned) }</p>
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
                            { for detail.items.iter().map(|item| html!{
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
