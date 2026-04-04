/// Notification preference settings page.
///
/// Available to all authenticated users.
///
/// Displays and updates:
/// - Per-type notification toggles (check-in, order, general)
/// - Do Not Disturb window (start / end times as HH:MM)
/// - Inbox frequency (immediate / daily / weekly)
use wasm_bindgen_futures::spawn_local;
use yew::prelude::*;
use yew_router::prelude::*;

use crate::api::preferences::{self, PatchPreferences};
use crate::router::Route;
use crate::state::AppStateContext;

// ---------------------------------------------------------------------------
// Page state
// ---------------------------------------------------------------------------

#[derive(Clone, PartialEq)]
enum PageState {
    Loading,
    Ready,
    Saving,
    Saved,
    Error(String),
}

// ---------------------------------------------------------------------------
// Component
// ---------------------------------------------------------------------------

#[function_component(PreferencesPage)]
pub fn preferences_page() -> Html {
    let state = use_context::<AppStateContext>().expect("AppState context not found");
    let navigator = use_navigator().unwrap();

    let is_auth = state.is_authenticated();

    {
        let navigator = navigator.clone();
        use_effect_with(is_auth, move |&auth| {
            if !auth {
                navigator.push(&Route::Login);
            }
        });
    }

    let page_state = use_state(|| PageState::Loading);
    // Individual form fields mirroring Preferences
    let notif_checkin = use_state(|| true);
    let notif_order = use_state(|| true);
    let notif_general = use_state(|| true);
    let dnd_enabled = use_state(|| false);
    let dnd_start = use_state(|| "21:00".to_string());
    let dnd_end = use_state(|| "06:00".to_string());
    let inbox_frequency = use_state(|| "immediate".to_string());

    // Load preferences on mount
    {
        let token = state.token.clone();
        let ps = page_state.clone();
        let nc = notif_checkin.clone();
        let no = notif_order.clone();
        let ng = notif_general.clone();
        let de = dnd_enabled.clone();
        let ds = dnd_start.clone();
        let dend = dnd_end.clone();
        let freq = inbox_frequency.clone();

        use_effect_with((), move |_| {
            let Some(token) = token else {
                ps.set(PageState::Error("Not authenticated.".into()));
                return;
            };
            spawn_local(async move {
                match preferences::get_preferences(&token).await {
                    Ok(p) => {
                        nc.set(p.notif_checkin);
                        no.set(p.notif_order);
                        ng.set(p.notif_general);
                        de.set(p.dnd_enabled);
                        ds.set(p.dnd_start);
                        dend.set(p.dnd_end);
                        freq.set(p.inbox_frequency);
                        ps.set(PageState::Ready);
                    }
                    Err(e) => {
                        ps.set(PageState::Error(format!("Failed to load preferences: {}", e)));
                    }
                }
            });
        });
    }

    // ── Save handler ──────────────────────────────────────────────────────
    let on_save = {
        let token = state.token.clone();
        let ps = page_state.clone();
        let nc = notif_checkin.clone();
        let no = notif_order.clone();
        let ng = notif_general.clone();
        let de = dnd_enabled.clone();
        let ds = dnd_start.clone();
        let dend = dnd_end.clone();
        let freq = inbox_frequency.clone();

        Callback::from(move |_: MouseEvent| {
            if matches!(*ps, PageState::Saving) {
                return;
            }
            let Some(ref token) = token else { return; };
            let token = token.clone();

            let patch = PatchPreferences {
                notif_checkin: Some(*nc),
                notif_order: Some(*no),
                notif_general: Some(*ng),
                dnd_enabled: Some(*de),
                dnd_start: Some((*ds).clone()),
                dnd_end: Some((*dend).clone()),
                inbox_frequency: Some((*freq).clone()),
            };

            let ps2 = ps.clone();
            let nc2 = nc.clone();
            let no2 = no.clone();
            let ng2 = ng.clone();
            let de2 = de.clone();
            let ds2 = ds.clone();
            let dend2 = dend.clone();
            let freq2 = freq.clone();

            ps.set(PageState::Saving);

            spawn_local(async move {
                match preferences::update_preferences(&patch, &token).await {
                    Ok(updated) => {
                        nc2.set(updated.notif_checkin);
                        no2.set(updated.notif_order);
                        ng2.set(updated.notif_general);
                        de2.set(updated.dnd_enabled);
                        ds2.set(updated.dnd_start);
                        dend2.set(updated.dnd_end);
                        freq2.set(updated.inbox_frequency);
                        ps2.set(PageState::Saved);
                    }
                    Err(e) => {
                        let msg = serde_json::from_str::<serde_json::Value>(&e.to_string())
                            .ok()
                            .and_then(|v| v["error"].as_str().map(|s| s.to_string()))
                            .unwrap_or_else(|| e.to_string());
                        ps2.set(PageState::Error(msg));
                    }
                }
            });
        })
    };

    if !is_auth {
        return html! { <div class="loading">{ "Checking access\u{2026}" }</div> };
    }

    match *page_state {
        PageState::Loading => html! { <div class="loading">{ "Loading preferences\u{2026}" }</div> },
        PageState::Error(ref msg) => html! {
            <div class="card">
                <p class="error-msg">{ msg.clone() }</p>
            </div>
        },
        _ => {
            let saving = matches!(*page_state, PageState::Saving);

            html! {
                <div>
                    <div class="card">
                        <h2>{ "Notification Preferences" }</h2>

                        if matches!(*page_state, PageState::Saved) {
                            <div class="banner banner-success">{ "Preferences saved." }</div>
                        }

                        // ── Notification toggles ──────────────────────────────
                        <section class="prefs-section">
                            <h3>{ "Notification Types" }</h3>

                            { toggle_row("Check-in notifications", *notif_checkin, {
                                let s = notif_checkin.clone();
                                Callback::from(move |_: MouseEvent| s.set(!*s))
                            }) }

                            { toggle_row("Order notifications", *notif_order, {
                                let s = notif_order.clone();
                                Callback::from(move |_: MouseEvent| s.set(!*s))
                            }) }

                            { toggle_row("General notifications", *notif_general, {
                                let s = notif_general.clone();
                                Callback::from(move |_: MouseEvent| s.set(!*s))
                            }) }
                        </section>

                        // ── Inbox frequency ───────────────────────────────────
                        <section class="prefs-section">
                            <h3>{ "Inbox Frequency" }</h3>
                            <p class="prefs-hint">
                                { "Controls when notifications appear in your inbox." }
                            </p>
                            <select
                                value={(*inbox_frequency).clone()}
                                onchange={{
                                    let freq = inbox_frequency.clone();
                                    Callback::from(move |e: Event| {
                                        let sel: web_sys::HtmlSelectElement = e.target_unchecked_into();
                                        freq.set(sel.value());
                                    })
                                }}
                            >
                                <option value="immediate">{ "Immediate — show as soon as received" }</option>
                                <option value="daily">{ "Daily digest — show at 5:00 PM" }</option>
                                <option value="weekly">{ "Weekly digest — show on Fridays at 5:00 PM" }</option>
                            </select>
                        </section>

                        // ── Do Not Disturb ────────────────────────────────────
                        <section class="prefs-section">
                            <h3>{ "Do Not Disturb" }</h3>
                            <p class="prefs-hint">
                                { "Non-critical notifications are deferred until the DND window ends. \
                                   Times are in your local timezone. Default: 9:00 PM – 6:00 AM." }
                            </p>

                            { toggle_row("Enable Do Not Disturb", *dnd_enabled, {
                                let s = dnd_enabled.clone();
                                Callback::from(move |_: MouseEvent| s.set(!*s))
                            }) }

                            if *dnd_enabled {
                                <div class="prefs-time-row">
                                    <label>{ "DND Start (HH:MM)" }</label>
                                    <input
                                        type="time"
                                        value={(*dnd_start).clone()}
                                        oninput={{
                                            let s = dnd_start.clone();
                                            Callback::from(move |e: InputEvent| {
                                                let inp: web_sys::HtmlInputElement = e.target_unchecked_into();
                                                s.set(inp.value());
                                            })
                                        }}
                                    />
                                </div>
                                <div class="prefs-time-row">
                                    <label>{ "DND End (HH:MM)" }</label>
                                    <input
                                        type="time"
                                        value={(*dnd_end).clone()}
                                        oninput={{
                                            let s = dnd_end.clone();
                                            Callback::from(move |e: InputEvent| {
                                                let inp: web_sys::HtmlInputElement = e.target_unchecked_into();
                                                s.set(inp.value());
                                            })
                                        }}
                                    />
                                </div>
                            }
                        </section>

                        <div class="prefs-actions">
                            <button
                                class="btn-primary"
                                onclick={on_save}
                                disabled={saving}
                            >
                                { if saving { "Saving\u{2026}" } else { "Save Preferences" } }
                            </button>
                        </div>
                    </div>
                </div>
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn toggle_row(label: &str, enabled: bool, on_click: Callback<MouseEvent>) -> Html {
    html! {
        <div class="prefs-toggle-row">
            <span class="toggle-label">{ label }</span>
            <button
                class={if enabled { "toggle-btn toggle-on" } else { "toggle-btn toggle-off" }}
                onclick={on_click}
                aria-pressed={enabled.to_string()}
            >
                { if enabled { "On" } else { "Off" } }
            </button>
        </div>
    }
}
