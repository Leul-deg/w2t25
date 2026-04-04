/// Admin product management page.
///
/// Lists all products (including inactive), allows creating new ones,
/// updating price/name, and deactivating (soft-delete).

use wasm_bindgen_futures::spawn_local;
use web_sys::HtmlInputElement;
use yew::prelude::*;

use crate::api::store::{
    self, CreateProductRequest, Product, UpdateProductRequest,
};
use crate::state::AppStateContext;

#[function_component(AdminProductsPage)]
pub fn admin_products_page() -> Html {
    let state = use_context::<AppStateContext>().expect("AppState context");
    let products = use_state(Vec::<Product>::new);
    let loading = use_state(|| true);
    let error = use_state(String::new);
    let success = use_state(String::new);
    let edit_values: UseStateHandle<std::collections::HashMap<String, (String, String, String)>> =
        use_state(std::collections::HashMap::new);

    // New product form fields
    let new_name = use_state(String::new);
    let new_price = use_state(String::new);
    let new_sku = use_state(String::new);
    let new_category = use_state(String::new);
    let new_qty = use_state(String::new);
    let creating = use_state(|| false);

    let reload = {
        let products = products.clone();
        let loading = loading.clone();
        let error = error.clone();
        let token = state.token.clone();
        Callback::from(move |_: ()| {
            let products = products.clone();
            let loading = loading.clone();
            let error = error.clone();
            let token = token.clone();
            loading.set(true);
            if let Some(token) = token {
                spawn_local(async move {
                    match store::admin_get_products(&token).await {
                        Ok(list) => {
                            products.set(list);
                            loading.set(false);
                        }
                        Err(e) => {
                            error.set(format!("Failed to load products: {}", e));
                            loading.set(false);
                        }
                    }
                });
            }
        })
    };

    // Initial load.
    {
        let reload = reload.clone();
        use_effect_with((), move |_| {
            reload.emit(());
        });
    }

    let on_create = {
        let state = state.clone();
        let new_name = new_name.clone();
        let new_price = new_price.clone();
        let new_sku = new_sku.clone();
        let new_category = new_category.clone();
        let new_qty = new_qty.clone();
        let error = error.clone();
        let success = success.clone();
        let creating = creating.clone();
        let reload = reload.clone();
        Callback::from(move |_: MouseEvent| {
            let token = match state.token.clone() {
                Some(t) => t,
                None => { error.set("Not authenticated.".to_string()); return; }
            };
            let name = new_name.trim().to_string();
            if name.is_empty() {
                error.set("Product name is required.".to_string());
                return;
            }
            let price_cents: i32 = match new_price.parse::<f64>() {
                Ok(p) => (p * 100.0).round() as i32,
                Err(_) => { error.set("Invalid price.".to_string()); return; }
            };
            let sku = if new_sku.trim().is_empty() { None } else { Some(new_sku.trim().to_string()) };
            let category = if new_category.trim().is_empty() { None } else { Some(new_category.trim().to_string()) };
            let initial_quantity = new_qty.parse::<i32>().ok();

            let req = CreateProductRequest {
                name,
                description: None,
                price_cents,
                sku,
                category,
                image_url: None,
                initial_quantity,
            };

            let creating = creating.clone();
            let error = error.clone();
            let success = success.clone();
            let reload = reload.clone();
            let new_name2 = new_name.clone();
            let new_price2 = new_price.clone();
            let new_sku2 = new_sku.clone();
            let new_category2 = new_category.clone();
            let new_qty2 = new_qty.clone();
            creating.set(true);
            spawn_local(async move {
                match store::admin_create_product(&token, &req).await {
                    Ok(p) => {
                        success.set(format!("Product '{}' created.", p.name));
                        error.set(String::new());
                        new_name2.set(String::new());
                        new_price2.set(String::new());
                        new_sku2.set(String::new());
                        new_category2.set(String::new());
                        new_qty2.set(String::new());
                        reload.emit(());
                    }
                    Err(e) => {
                        error.set(format!("Create failed: {}", e));
                        success.set(String::new());
                    }
                }
                creating.set(false);
            });
        })
    };

    html! {
        <div class="page-container">
            <h2>{ "Product Management" }</h2>

            if !error.is_empty() {
                <div class="error-banner">{ error.as_str() }</div>
            }
            if !success.is_empty() {
                <div class="success-banner">{ success.as_str() }</div>
            }

            // ── Create product form ───────────────────────────────────────
            <div class="card">
                <h3>{ "Add New Product" }</h3>
                <div class="form-row">
                    <label>{ "Name *" }</label>
                    <input
                        type="text"
                        value={(*new_name).clone()}
                        oninput={
                            let s = new_name.clone();
                            Callback::from(move |e: InputEvent| {
                                let el: HtmlInputElement = e.target_unchecked_into();
                                s.set(el.value());
                            })
                        }
                        placeholder="Product name"
                    />
                </div>
                <div class="form-row">
                    <label>{ "Price ($)" }</label>
                    <input
                        type="number"
                        step="0.01"
                        min="0"
                        value={(*new_price).clone()}
                        oninput={
                            let s = new_price.clone();
                            Callback::from(move |e: InputEvent| {
                                let el: HtmlInputElement = e.target_unchecked_into();
                                s.set(el.value());
                            })
                        }
                        placeholder="19.99"
                    />
                </div>
                <div class="form-row">
                    <label>{ "SKU" }</label>
                    <input
                        type="text"
                        value={(*new_sku).clone()}
                        oninput={
                            let s = new_sku.clone();
                            Callback::from(move |e: InputEvent| {
                                let el: HtmlInputElement = e.target_unchecked_into();
                                s.set(el.value());
                            })
                        }
                        placeholder="HOO-001"
                    />
                </div>
                <div class="form-row">
                    <label>{ "Category" }</label>
                    <input
                        type="text"
                        value={(*new_category).clone()}
                        oninput={
                            let s = new_category.clone();
                            Callback::from(move |e: InputEvent| {
                                let el: HtmlInputElement = e.target_unchecked_into();
                                s.set(el.value());
                            })
                        }
                        placeholder="apparel"
                    />
                </div>
                <div class="form-row">
                    <label>{ "Initial Stock" }</label>
                    <input
                        type="number"
                        min="0"
                        value={(*new_qty).clone()}
                        oninput={
                            let s = new_qty.clone();
                            Callback::from(move |e: InputEvent| {
                                let el: HtmlInputElement = e.target_unchecked_into();
                                s.set(el.value());
                            })
                        }
                        placeholder="0"
                    />
                </div>
                <button
                    class="btn btn-primary"
                    onclick={on_create}
                    disabled={*creating}
                >
                    if *creating { { "Creating\u{2026}" } } else { { "Create Product" } }
                </button>
            </div>

            // ── Product table ─────────────────────────────────────────────
            if *loading {
                <p>{ "Loading\u{2026}" }</p>
            } else {
                <table class="data-table">
                    <thead>
                        <tr>
                            <th>{ "Name" }</th>
                            <th>{ "SKU" }</th>
                            <th>{ "Category" }</th>
                            <th>{ "Price ($)" }</th>
                            <th>{ "Stock" }</th>
                            <th>{ "Threshold" }</th>
                            <th>{ "Active" }</th>
                            <th>{ "Actions" }</th>
                        </tr>
                    </thead>
                    <tbody>
                        { products.iter().map(|p| {
                            let p = p.clone();
                            let token = state.token.clone().unwrap_or_default();
                            let error2 = error.clone();
                            let success2 = success.clone();
                            let reload2 = reload.clone();
                            let edit_values = edit_values.clone();
                            let key = p.id.to_string();
                            let (price_edit, qty_edit, threshold_edit) = edit_values
                                .get(&key)
                                .cloned()
                                .unwrap_or_else(|| (
                                    format!("{:.2}", p.price_cents as f64 / 100.0),
                                    p.quantity.unwrap_or(0).to_string(),
                                    p.low_stock_threshold.unwrap_or(10).to_string(),
                                ));

                            let on_deactivate = {
                                let p = p.clone();
                                let token = token.clone();
                                let error2 = error2.clone();
                                let success2 = success2.clone();
                                let reload2 = reload2.clone();
                                Callback::from(move |_: MouseEvent| {
                                    let pid = p.id;
                                    let pname = p.name.clone();
                                    let token = token.clone();
                                    let error2 = error2.clone();
                                    let success2 = success2.clone();
                                    let reload2 = reload2.clone();
                                    spawn_local(async move {
                                        match store::admin_deactivate_product(&token, &pid).await {
                                            Ok(_) => {
                                                success2.set(format!("'{}' deactivated.", pname));
                                                reload2.emit(());
                                            }
                                            Err(e) => {
                                                error2.set(format!("Deactivate failed: {}", e));
                                            }
                                        }
                                    });
                                })
                            };

                            let on_price_input = {
                                let edit_values = edit_values.clone();
                                let qty_edit = qty_edit.clone();
                                let threshold_edit = threshold_edit.clone();
                                let key = key.clone();
                                Callback::from(move |e: InputEvent| {
                                    let el: HtmlInputElement = e.target_unchecked_into();
                                    let mut map = (*edit_values).clone();
                                    map.insert(key.clone(), (el.value(), qty_edit.clone(), threshold_edit.clone()));
                                    edit_values.set(map);
                                })
                            };

                            let on_qty_input = {
                                let edit_values = edit_values.clone();
                                let price_edit = price_edit.clone();
                                let threshold_edit = threshold_edit.clone();
                                let key = key.clone();
                                Callback::from(move |e: InputEvent| {
                                    let el: HtmlInputElement = e.target_unchecked_into();
                                    let mut map = (*edit_values).clone();
                                    map.insert(key.clone(), (price_edit.clone(), el.value(), threshold_edit.clone()));
                                    edit_values.set(map);
                                })
                            };

                            let on_threshold_input = {
                                let edit_values = edit_values.clone();
                                let price_edit = price_edit.clone();
                                let qty_edit = qty_edit.clone();
                                let key = key.clone();
                                Callback::from(move |e: InputEvent| {
                                    let el: HtmlInputElement = e.target_unchecked_into();
                                    let mut map = (*edit_values).clone();
                                    map.insert(key.clone(), (price_edit.clone(), qty_edit.clone(), el.value()));
                                    edit_values.set(map);
                                })
                            };

                            let on_save = {
                                let token = token.clone();
                                let error2 = error2.clone();
                                let success2 = success2.clone();
                                let reload2 = reload2.clone();
                                let edit_values = edit_values.clone();
                                let key = key.clone();
                                let product_id = p.id;
                                let product_name = p.name.clone();
                                Callback::from(move |_: MouseEvent| {
                                    let (price, qty, threshold) = edit_values
                                        .get(&key)
                                        .cloned()
                                        .unwrap_or_default();
                                    let price_cents = match price.parse::<f64>() {
                                        Ok(v) => (v * 100.0).round() as i32,
                                        Err(_) => {
                                            error2.set("Invalid price.".to_string());
                                            return;
                                        }
                                    };
                                    let quantity = match qty.parse::<i32>() {
                                        Ok(v) => v,
                                        Err(_) => {
                                            error2.set("Invalid quantity.".to_string());
                                            return;
                                        }
                                    };
                                    let low_stock_threshold = match threshold.parse::<i32>() {
                                        Ok(v) => v,
                                        Err(_) => {
                                            error2.set("Invalid threshold.".to_string());
                                            return;
                                        }
                                    };
                                    let req = UpdateProductRequest {
                                        name: None,
                                        description: None,
                                        price_cents: Some(price_cents),
                                        sku: None,
                                        category: None,
                                        image_url: None,
                                        active: None,
                                        quantity: Some(quantity),
                                        low_stock_threshold: Some(low_stock_threshold),
                                    };
                                    let token = token.clone();
                                    let error2 = error2.clone();
                                    let success2 = success2.clone();
                                    let reload2 = reload2.clone();
                                    let product_name = product_name.clone();
                                    spawn_local(async move {
                                        match store::admin_update_product(&token, &product_id, &req).await {
                                            Ok(_) => {
                                                success2.set(format!("Updated '{}'.", product_name));
                                                error2.set(String::new());
                                                reload2.emit(());
                                            }
                                            Err(e) => error2.set(format!("Update failed: {}", e)),
                                        }
                                    });
                                })
                            };

                            let stock_display = match p.quantity {
                                Some(q) if q < p.low_stock_threshold.unwrap_or(10) && q > 0 =>
                                    format!("{} ⚠", q),
                                Some(q) => q.to_string(),
                                None => "—".to_string(),
                            };

                            html! {
                                <tr key={p.id.to_string()}>
                                    <td>{ &p.name }</td>
                                    <td>{ p.sku.as_deref().unwrap_or("—") }</td>
                                    <td>{ p.category.as_deref().unwrap_or("—") }</td>
                                    <td>
                                        <input class="inline-input compact-input" type="number" step="0.01" min="0" value={price_edit} oninput={on_price_input} />
                                    </td>
                                    <td>
                                        <div class="stacked-inline">
                                            <span>{ stock_display }</span>
                                            <input class="inline-input compact-input" type="number" min="0" value={qty_edit} oninput={on_qty_input} />
                                        </div>
                                    </td>
                                    <td>
                                        <input class="inline-input compact-input" type="number" min="1" value={threshold_edit} oninput={on_threshold_input} />
                                    </td>
                                    <td>{ if p.active { "Yes" } else { "No" } }</td>
                                    <td>
                                        <button class="btn-sm btn-primary" onclick={on_save}>
                                            { "Save" }
                                        </button>
                                        if p.active {
                                            <button
                                                class="btn-sm btn-danger"
                                                onclick={on_deactivate}
                                            >
                                                { "Deactivate" }
                                            </button>
                                        }
                                    </td>
                                </tr>
                            }
                        }).collect::<Html>() }
                    </tbody>
                </table>
            }
        </div>
    }
}
