/// In-app notification inbox.
///
/// Shows the calling user's notifications (newest first).
/// Clicking an unread notification marks it read.
use yew::prelude::*;
use yew_router::prelude::*;
use wasm_bindgen_futures::spawn_local;

use crate::api::notifications::{self, Notification};
use crate::router::Route;
use crate::state::AppStateContext;

#[function_component(InboxPage)]
pub fn inbox_page() -> Html {
    let state = use_context::<AppStateContext>().expect("AppState context not found");
    let navigator = use_navigator().unwrap();

    let is_auth = state.is_authenticated();

    {
        let navigator = navigator.clone();
        use_effect_with(is_auth, move |auth| {
            if !auth {
                navigator.push(&Route::Login);
            }
        });
    }

    let notifications = use_state(Vec::<Notification>::new);
    let loaded = use_state(|| false);
    let page_error = use_state(|| Option::<String>::None);

    // Load notifications on mount
    {
        let token = state.token.clone();
        let notifs = notifications.clone();
        let loaded = loaded.clone();
        let page_error = page_error.clone();

        use_effect_with((), move |_| {
            let Some(token) = token else { return; };
            let notifs2 = notifs.clone();
            let loaded2 = loaded.clone();
            let page_error2 = page_error.clone();
            spawn_local(async move {
                let _ = notifications::generate_reminders(&token).await;
                if let Ok(ns) = notifications::list_notifications(&token).await {
                    notifs2.set(ns);
                    page_error2.set(None);
                } else {
                    page_error2.set(Some("Failed to load inbox items.".to_string()));
                }
                loaded2.set(true);
            });
        });
    }

    // Mark a notification as read and update local state
    let mark_read = {
        let token = state.token.clone();
        let notifs = notifications.clone();

        move |notification_id: String| {
            let token = token.clone();
            let notifs = notifs.clone();
            let nid = notification_id.clone();

            Callback::from(move |_: MouseEvent| {
                let Some(ref tok) = token else { return; };
                let tok = tok.clone();
                let notifs2 = notifs.clone();
                let nid2 = nid.clone();

                spawn_local(async move {
                    let _ = notifications::mark_read(&nid2, &tok).await;
                    // Update local state: set read_at on the notification
                    let updated: Vec<Notification> = (*notifs2)
                        .iter()
                        .map(|n| {
                            if n.id == nid2 && n.read_at.is_none() {
                                Notification {
                                    read_at: Some("now".to_string()),
                                    ..n.clone()
                                }
                            } else {
                                n.clone()
                            }
                        })
                        .collect();
                    notifs2.set(updated);
                });
            })
        }
    };

    if !is_auth {
        return html! { <div class="loading">{ "Checking access\u{2026}" }</div> };
    }

    let unread_count = (*notifications).iter().filter(|n| n.read_at.is_none()).count();

    html! {
        <div>
            <div class="card">
                <h2>
                    { "Inbox" }
                    if unread_count > 0 {
                        <span class="unread-badge">{ format!(" {} unread", unread_count) }</span>
                    }
                </h2>
            </div>

            if !*loaded {
                <div class="card"><p>{ "Loading\u{2026}" }</p></div>
            } else if let Some(ref err) = *page_error {
                <div class="card"><p class="error-msg">{ err.clone() }</p></div>
            } else if (*notifications).is_empty() {
                <div class="card"><p>{ "Your inbox is empty." }</p></div>
            } else {
                <div class="notification-list">
                    { for (*notifications).iter().map(|n| {
                        let is_unread = n.read_at.is_none();
                        let nid = n.id.clone();
                        let on_click = mark_read(nid);
                        let type_css = notif_type_css(&n.notification_type);
                        html! {
                            <div
                                key={n.id.clone()}
                                class={if is_unread {
                                    "notification-item unread"
                                } else {
                                    "notification-item"
                                }}
                                onclick={on_click}
                            >
                                <div class="notif-header">
                                    <span class={type_css}>{ notif_type_label(&n.notification_type) }</span>
                                    <span class="notif-time">{ fmt_time(&n.created_at) }</span>
                                    if is_unread {
                                        <span class="unread-dot" title="Unread" />
                                    }
                                </div>
                                <div class="notif-subject">{ &n.subject }</div>
                                <div class="notif-body">{ &n.body }</div>
                                if let Some(ref sender) = n.sender_username {
                                    <div class="notif-sender">{ format!("From: {}", sender) }</div>
                                }
                            </div>
                        }
                    }) }
                </div>
            }
        </div>
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn notif_type_css(t: &str) -> &'static str {
    match t {
        "checkin" => "notif-type checkin",
        "order" => "notif-type order",
        "alert" => "notif-type alert",
        "system" => "notif-type system",
        _ => "notif-type general",
    }
}

fn notif_type_label(t: &str) -> &'static str {
    match t {
        "checkin" => "Check-In",
        "order" => "Order",
        "alert" => "Alert",
        "system" => "System",
        _ => "General",
    }
}

fn fmt_time(iso: &str) -> String {
    iso.get(..16)
        .map(|s| s.replace('T', " "))
        .unwrap_or_else(|| iso.to_string())
}

#[cfg(test)]
mod tests {
    use super::{fmt_time, notif_type_css, notif_type_label};

    // ---------------------------------------------------------------------------
    // notif_type_css
    // ---------------------------------------------------------------------------

    #[test]
    fn notif_type_css_checkin() {
        assert_eq!(notif_type_css("checkin"), "notif-type checkin");
    }

    #[test]
    fn notif_type_css_order() {
        assert_eq!(notif_type_css("order"), "notif-type order");
    }

    #[test]
    fn notif_type_css_alert() {
        assert_eq!(notif_type_css("alert"), "notif-type alert");
    }

    #[test]
    fn notif_type_css_system() {
        assert_eq!(notif_type_css("system"), "notif-type system");
    }

    #[test]
    fn notif_type_css_unknown_falls_back_to_general() {
        assert_eq!(notif_type_css("announcement"), "notif-type general");
        assert_eq!(notif_type_css(""), "notif-type general");
    }

    // ---------------------------------------------------------------------------
    // notif_type_label
    // ---------------------------------------------------------------------------

    #[test]
    fn notif_type_label_checkin() {
        assert_eq!(notif_type_label("checkin"), "Check-In");
    }

    #[test]
    fn notif_type_label_order() {
        assert_eq!(notif_type_label("order"), "Order");
    }

    #[test]
    fn notif_type_label_alert() {
        assert_eq!(notif_type_label("alert"), "Alert");
    }

    #[test]
    fn notif_type_label_system() {
        assert_eq!(notif_type_label("system"), "System");
    }

    #[test]
    fn notif_type_label_unknown_falls_back_to_general() {
        assert_eq!(notif_type_label("promotion"), "General");
        assert_eq!(notif_type_label(""), "General");
    }

    // ---------------------------------------------------------------------------
    // fmt_time
    // ---------------------------------------------------------------------------

    #[test]
    fn fmt_time_replaces_t_in_iso_string() {
        assert_eq!(fmt_time("2026-04-01T14:30:00Z"), "2026-04-01 14:30");
    }

    #[test]
    fn fmt_time_truncates_to_16_chars() {
        // Exactly 16 chars after the 'T' replacement
        let result = fmt_time("2026-01-15T08:05:00.000Z");
        assert_eq!(result, "2026-01-15 08:05");
    }

    #[test]
    fn fmt_time_short_string_returns_original() {
        // String shorter than 16 chars cannot be sliced — returns original
        let short = "2026-04-01T";
        let result = fmt_time(short);
        assert_eq!(result, short);
    }

    #[test]
    fn fmt_time_empty_string_returns_empty() {
        assert_eq!(fmt_time(""), "");
    }
}
