/// Merch store page: product listing, in-page cart, and checkout.
///
/// All three phases live on one route (/store) so cart state never crosses
/// a navigation boundary.
///
/// Flow:
///   1. Products load on mount.
///   2. User sets quantities and clicks "Add to Cart".
///   3. Cart summary appears; user clicks "Place Order".
///   4. POST /orders is called; success shows confirmation.

use wasm_bindgen_futures::spawn_local;
use web_sys::HtmlInputElement;
use yew::prelude::*;

use crate::api::store::{
    self, CartItem, CommerceConfigSummary, CreateOrderRequest, OrderDetail, OrderLineInput, Product,
};
use crate::state::AppStateContext;

#[derive(Clone, PartialEq)]
enum StoreView {
    Browsing,
    OrderConfirmed(OrderDetail),
}

#[function_component(StorePage)]
pub fn store_page() -> Html {
    let state = use_context::<AppStateContext>().expect("AppState context");
    let products = use_state(Vec::<Product>::new);
    let cart = use_state(Vec::<CartItem>::new);
    let commerce_config = use_state(|| None::<CommerceConfigSummary>);
    let error = use_state(String::new);
    let loading = use_state(|| true);
    let placing = use_state(|| false);
    let view = use_state(|| StoreView::Browsing);
    // Quantity inputs keyed by product_id string
    let quantities: UseStateHandle<std::collections::HashMap<String, i32>> =
        use_state(std::collections::HashMap::new);

    // Load products on mount.
    {
        let products = products.clone();
        let loading = loading.clone();
        let error = error.clone();
        let commerce_config = commerce_config.clone();
        let token = state.token.clone();
        use_effect_with((), move |_| {
            if let Some(token) = token {
                spawn_local(async move {
                    let products_result = store::get_products(&token).await;
                    let config_result = store::get_commerce_config(&token).await;

                    match products_result {
                        Ok(list) => {
                            products.set(list);
                        }
                        Err(e) => {
                            error.set(format!("Failed to load products: {}", e));
                        }
                    }
                    if let Ok(config) = config_result {
                        commerce_config.set(Some(config));
                    }
                    loading.set(false);
                });
            } else {
                error.set("Not authenticated.".to_string());
                loading.set(false);
            }
        });
    }

    if *loading {
        return html! { <div class="card"><p>{ "Loading store\u{2026}" }</p></div> };
    }

    if let StoreView::OrderConfirmed(ref order) = *view {
        return render_confirmation(order);
    }

    let cart_subtotal: i32 = cart.iter().map(|ci| ci.subtotal_cents()).sum();
    let shipping_fee_cents = commerce_config
        .as_ref()
        .map(|c| c.shipping_fee_cents as i32)
        .unwrap_or(695);
    let points_rate = commerce_config
        .as_ref()
        .map(|c| c.points_rate_per_dollar as i32)
        .unwrap_or(1);
    let shipping = shipping_fee_cents as f64 / 100.0;
    let cart_total = cart_subtotal as f64 / 100.0 + shipping;
    let points_preview = (cart_subtotal / 100) * points_rate;
    let on_checkout = {
        let token = state.token.clone();
        let cart3 = cart.clone();
        let placing2 = placing.clone();
        let error2 = error.clone();
        let view2 = view.clone();
        Callback::from(move |_: MouseEvent| {
            let token = match token.clone() {
                Some(t) => t,
                None => {
                    error2.set("Not authenticated.".to_string());
                    return;
                }
            };
            let items = cart3
                .iter()
                .map(|ci| OrderLineInput {
                    product_id: ci.product_id,
                    quantity: ci.quantity,
                })
                .collect();
            let req = CreateOrderRequest { items, notes: None };
            let placing2 = placing2.clone();
            let error2 = error2.clone();
            let view2 = view2.clone();
            let cart3 = cart3.clone();
            placing2.set(true);
            spawn_local(async move {
                match store::create_order(&token, &req).await {
                    Ok(order) => {
                        cart3.set(vec![]);
                        view2.set(StoreView::OrderConfirmed(order));
                    }
                    Err(e) => {
                        error2.set(format!("Order failed: {}", e));
                    }
                }
                placing2.set(false);
            });
        })
    };

    html! {
        <div class="page-container">
            <h2>{ "Merch Store" }</h2>

            if let Some(config) = &*commerce_config {
                <div class="card">
                    <h3>{ "Store Details" }</h3>
                    <p>{ format!(
                        "Shipping fee: {}. Points rate: {} point(s) per $1.00.",
                        config.shipping_fee_display,
                        config.points_rate_per_dollar
                    ) }</p>
                    if !config.campaigns.is_empty() {
                        <div class="campaign-chip-row">
                            { for config.campaigns.iter().map(|campaign| html! {
                                <span
                                    key={campaign.name.clone()}
                                    class={if campaign.enabled { "badge badge-success" } else { "badge badge-default" }}
                                >
                                    { format!("{}: {}", campaign.name, if campaign.enabled { "on" } else { "off" }) }
                                </span>
                            }) }
                        </div>
                    }
                </div>
            }

            if !error.is_empty() {
                <div class="error-banner">{ error.as_str() }</div>
            }

            <div class="store-layout">
                // ── Product grid ──────────────────────────────────────────
                <section class="product-grid">
                    { products.iter().map(|p| {
                        let p = p.clone();
                        let quantities = quantities.clone();
                        let cart = cart.clone();
                        let product_id_str = p.id.to_string();

                        let on_qty_input = {
                            let quantities = quantities.clone();
                            let pid = product_id_str.clone();
                            Callback::from(move |e: InputEvent| {
                                let input: HtmlInputElement = e.target_unchecked_into();
                                let val: i32 = input.value().parse().unwrap_or(1).max(1).min(99);
                                let mut map = (*quantities).clone();
                                map.insert(pid.clone(), val);
                                quantities.set(map);
                            })
                        };

                        let on_add = {
                            let p = p.clone();
                            let quantities = quantities.clone();
                            let cart = cart.clone();
                            let pid = product_id_str.clone();
                            Callback::from(move |_: MouseEvent| {
                                let qty = quantities.get(&pid).copied().unwrap_or(1);
                                let mut updated = (*cart).clone();
                                if let Some(existing) = updated.iter_mut().find(|c| c.product_id == p.id) {
                                    existing.quantity += qty;
                                } else {
                                    updated.push(CartItem {
                                        product_id: p.id,
                                        product_name: p.name.clone(),
                                        quantity: qty,
                                        unit_price_cents: p.price_cents,
                                    });
                                }
                                cart.set(updated);
                            })
                        };

                        let stock_label = if !p.in_stock() {
                            html! { <span class="badge badge-danger">{ "Out of Stock" }</span> }
                        } else if p.low_stock() {
                            html! { <span class="badge badge-warning">{ format!("Only {} left", p.quantity.unwrap_or(0)) }</span> }
                        } else {
                            html! { <span class="badge badge-success">{ format!("{} in stock", p.quantity.unwrap_or(0)) }</span> }
                        };

                        let current_qty = quantities.get(&product_id_str).copied().unwrap_or(1);

                        html! {
                            <div class="product-card" key={p.id.to_string()}>
                                if let Some(cat) = &p.category {
                                    <div class="product-category">{ cat }</div>
                                }
                                <h3>{ &p.name }</h3>
                                if let Some(desc) = &p.description {
                                    <p class="product-desc">{ desc }</p>
                                }
                                <div class="product-price">{ p.price_display() }</div>
                                { stock_label }
                                if p.in_stock() {
                                    <div class="product-actions">
                                        <input
                                            type="number"
                                            min="1"
                                            max="99"
                                            value={current_qty.to_string()}
                                            oninput={on_qty_input}
                                            class="qty-input"
                                        />
                                        <button
                                            class="btn btn-primary"
                                            onclick={on_add}
                                        >
                                            { "Add to Cart" }
                                        </button>
                                    </div>
                                }
                            </div>
                        }
                    }).collect::<Html>() }
                </section>

                // ── Cart sidebar ──────────────────────────────────────────
                <aside class="cart-sidebar">
                    <h3>{ "Your Cart" }</h3>
                    if cart.is_empty() {
                        <p class="empty-cart">{ "Your cart is empty." }</p>
                    } else {
                        <table class="cart-table">
                            <thead>
                                <tr>
                                    <th>{ "Item" }</th>
                                    <th>{ "Qty" }</th>
                                    <th>{ "Subtotal" }</th>
                                    <th></th>
                                </tr>
                            </thead>
                            <tbody>
                                { cart.iter().enumerate().map(|(idx, item)| {
                                    let cart2 = cart.clone();
                                    let on_remove = Callback::from(move |_: MouseEvent| {
                                        let mut updated = (*cart2).clone();
                                        updated.remove(idx);
                                        cart2.set(updated);
                                    });
                                    html! {
                                        <tr key={item.product_id.to_string()}>
                                            <td>{ &item.product_name }</td>
                                            <td>{ item.quantity }</td>
                                            <td>{ item.subtotal_display() }</td>
                                            <td>
                                                <button class="btn-sm btn-danger" onclick={on_remove}>
                                                    { "\u{00d7}" }
                                                </button>
                                            </td>
                                        </tr>
                                    }
                                }).collect::<Html>() }
                            </tbody>
                        </table>

                        <div class="cart-totals">
                            <div class="cart-row">
                                <span>{ "Subtotal" }</span>
                                <span>{ format!("${:.2}", cart_subtotal as f64 / 100.0) }</span>
                            </div>
                            <div class="cart-row">
                                <span>{ "Shipping" }</span>
                                <span>{ format!("${:.2}", shipping) }</span>
                            </div>
                            <div class="cart-row cart-total">
                                <span>{ "Total (est.)" }</span>
                                <span>{ format!("${:.2}", cart_total) }</span>
                            </div>
                            <div class="cart-row cart-points">
                                <span>{ "Points you'll earn" }</span>
                                <span>{ format!("{} pts", points_preview) }</span>
                            </div>
                        </div>

                        <button
                            class="btn btn-success btn-full"
                            onclick={on_checkout}
                            disabled={*placing}
                        >
                            if *placing {
                                { "Placing order\u{2026}" }
                            } else {
                                { "Place Order" }
                            }
                        </button>
                    }
                </aside>
            </div>
        </div>
    }
}

fn render_confirmation(order: &OrderDetail) -> Html {
    html! {
        <div class="page-container">
            <div class="card order-confirm">
                <h2>{ "\u{2705} Order Placed!" }</h2>
                <p>{ format!("Order ID: {}", order.id) }</p>
                <p>{ format!("Status: {}", order.status) }</p>
                <p>{ format!("Total: ${:.2}", order.total_cents as f64 / 100.0) }</p>
                <p>{ format!("Shipping: ${:.2}", order.shipping_fee_cents as f64 / 100.0) }</p>
                <p>{ format!("Points earned: {}", order.points_earned) }</p>
                <h3>{ "Items" }</h3>
                <table class="cart-table">
                    <thead>
                        <tr>
                            <th>{ "Product" }</th>
                            <th>{ "Qty" }</th>
                            <th>{ "Subtotal" }</th>
                        </tr>
                    </thead>
                    <tbody>
                        { order.items.iter().map(|item| html! {
                            <tr key={item.id.to_string()}>
                                <td>{ &item.product_name }</td>
                                <td>{ item.quantity }</td>
                                <td>{ item.subtotal_display() }</td>
                            </tr>
                        }).collect::<Html>() }
                    </tbody>
                </table>
                <p class="hint">{ "Your order has been confirmed. Check your inbox for details." }</p>
            </div>
        </div>
    }
}
