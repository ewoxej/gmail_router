use crate::api::{
    create_api_token, list_addresses, list_api_tokens, revoke_api_token, router_status, scan_now,
    set_action,
};
use crate::models::{AddressEntry, ApiTokenRow, RouteAction};
use dioxus::prelude::*;

const MAIN_CSS: Asset = asset!("/assets/main.css");

#[component]
pub fn Home() -> Element {
    let mut reload = use_signal(|| 0u32);
    let mut addresses = use_signal(Vec::<AddressEntry>::new);
    let mut scanning = use_signal(|| false);
    let mut error = use_signal(|| Option::<String>::None);

    let status = use_resource(move || async move {
        reload();
        router_status().await.unwrap_or_default()
    });

    let _addr_loader = use_resource(move || async move {
        reload();
        match list_addresses().await {
            Ok(list) => addresses.set(list),
            Err(e) => error.set(Some(format!("Failed to load addresses: {e}"))),
        }
    });

    let loading = status.read().is_none();
    let st = status().unwrap_or_default();
    let domain = st.domain.clone();
    
    let kept = addresses.read().iter().filter(|a| a.action.is_keep()).count();
    let routed = addresses.read().len().saturating_sub(kept);

    let decision_mode = st.mode == "decision";
    let show_manager = decision_mode || st.authenticated;

    rsx! {
        document::Link { rel: "stylesheet", href: MAIN_CSS }
        main { class: "wrap",
            header { class: "hdr",
                h1 { "Gmail Router" }
                if !domain.is_empty() {
                    p { class: "muted",
                        "Routing "
                        code { "@{domain}" }
                    }
                }
            }

            if let Some(msg) = error() {
                p { class: "err", "{msg}" }
            }

            if loading {
                p { class: "muted", "Loading…" }
            } else if !show_manager {
                SignIn {}
            } else {
                section { class: "panel",
                    div { class: "row between",
                        div { class: "counts",
                            span { class: "pill ok", "{kept} kept" }
                            span { class: "pill block", "{routed} routed" }
                            if decision_mode {
                                span { class: "pill mode", "decision mode" }
                            } else if let Some(last) = st.last_scan.clone() {
                                span { class: "muted small", "last scan: {last}" }
                            }
                        }
                        if !decision_mode {
                            button {
                                class: "btn",
                                disabled: scanning(),
                                onclick: move |_| {
                                    scanning.set(true);
                                    error.set(None);
                                    spawn(async move {
                                        match scan_now().await {
                                            Ok(_) => reload.set(reload() + 1),
                                            Err(e) => error.set(Some(format!("Scan failed: {e}"))),
                                        }
                                        scanning.set(false);
                                    });
                                },
                                if scanning() {
                                    "Scanning…"
                                } else {
                                    "Scan now"
                                }
                            }
                        }
                    }

                    if decision_mode {
                        p { class: "muted small",
                            "Decision mode: this app only stores policy. Point your worker at "
                            code { "GET /api/route?address=…" }
                            " with an API token. Unknown addresses default to Keep and are added here automatically."
                        }
                    } else {
                        p { class: "muted small",
                            "Keep = routed through untouched. The other actions apply to future mail sent to that address."
                        }
                    }

                    if addresses.read().is_empty() {
                        p { class: "muted",
                            if decision_mode {
                                "No addresses yet — they'll appear as your worker asks about incoming recipients."
                            } else {
                                "No addresses yet. Run a scan to discover recipients."
                            }
                        }
                    } else {
                        ul { class: "list",
                            for entry in addresses.read().iter().cloned() {
                                AddressRow {
                                    key: "{entry.local_part}",
                                    local_part: entry.local_part.clone(),
                                    domain: domain.clone(),
                                    action: entry.action.clone(),
                                    on_change: move |action: RouteAction| {
                                        let local = entry.local_part.clone();
                                        let pos = addresses
                                            .read()
                                            .iter()
                                            .position(|a| a.local_part == local);
                                        if let Some(pos) = pos {
                                            addresses.write()[pos].action = action.clone();
                                        }
                                        spawn(async move {
                                            if let Err(e) = set_action(local.clone(), action).await {
                                                error.set(Some(format!("Failed to update {local}: {e}")));
                                            }
                                        });
                                    },
                                }
                            }
                        }
                    }
                }
                TokensPanel {}
            }
        }
    }
}

#[component]
fn TokensPanel() -> Element {
    let mut reload = use_signal(|| 0u32);
    let mut name = use_signal(String::new);
    let mut created = use_signal(|| Option::<String>::None);
    let mut error = use_signal(|| Option::<String>::None);

    let tokens = use_resource(move || async move {
        reload();
        list_api_tokens().await.unwrap_or_default()
    });
    let list: Vec<ApiTokenRow> = tokens().unwrap_or_default();

    rsx! {
        section { class: "panel",
            h2 { class: "h2", "API tokens" }
            p { class: "muted small",
                "Use a token as "
                code { "Authorization: Bearer <token>" }
                " to drive the routing API without a browser login."
            }

            if let Some(msg) = error() {
                p { class: "err", "{msg}" }
            }
            if let Some(tok) = created() {
                div { class: "token-new",
                    p { class: "small", "New token (copy it now — it won't be shown again):" }
                    code { class: "token-val", "{tok}" }
                }
            }

            div { class: "row token-form",
                input {
                    class: "fwd-input",
                    r#type: "text",
                    placeholder: "token name (e.g. automation)",
                    value: "{name}",
                    oninput: move |evt| name.set(evt.value()),
                }
                button {
                    class: "btn",
                    onclick: move |_| {
                        let n = name();
                        error.set(None);
                        spawn(async move {
                            match create_api_token(n).await {
                                Ok(raw) => {
                                    created.set(Some(raw));
                                    name.set(String::new());
                                    reload.set(reload() + 1);
                                }
                                Err(e) => error.set(Some(format!("Create failed: {e}"))),
                            }
                        });
                    },
                    "Create"
                }
            }

            if list.is_empty() {
                p { class: "muted small", "No tokens yet." }
            } else {
                ul { class: "list",
                    for tok in list.iter().cloned() {
                        li { key: "{tok.id}", class: "item",
                            div { class: "row between",
                                div {
                                    span { class: "addr", "{tok.name}" }
                                    span { class: "muted small",
                                        " · created {tok.created_at}"
                                        if let Some(used) = tok.last_used_at.clone() {
                                            " · last used {used}"
                                        }
                                    }
                                }
                                button {
                                    class: "btn danger",
                                    onclick: move |_| {
                                        let id = tok.id;
                                        spawn(async move {
                                            if let Err(e) = revoke_api_token(id).await {
                                                error.set(Some(format!("Revoke failed: {e}")));
                                            } else {
                                                reload.set(reload() + 1);
                                            }
                                        });
                                    },
                                    "Revoke"
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn AddressRow(
    local_part: String,
    domain: String,
    action: RouteAction,
    on_change: EventHandler<RouteAction>,
) -> Element {
    let mut kind = use_signal(|| action.kind().to_string());
    let mut forward_to = use_signal(|| action.forward_to().to_string());

    let is_keep = kind() == "keep";

    rsx! {
        li { class: if is_keep { "item" } else { "item off" },
            div { class: "row addr-row",
                span { class: "addr",
                    "{local_part}"
                    if !domain.is_empty() {
                        span { class: "at", "@{domain}" }
                    }
                }
                select {
                    class: "act-select",
                    value: "{kind}",
                    onchange: move |evt| {
                        let k = evt.value();
                        kind.set(k.clone());
                        if k == "forward" {
                            let to = forward_to();
                            if !to.trim().is_empty() {
                                on_change.call(RouteAction::from_kind("forward", &to));
                            }
                        } else {
                            on_change.call(RouteAction::from_kind(&k, ""));
                        }
                    },
                    option { value: "keep", "Keep" }
                    option { value: "trash", "Trash" }
                    option { value: "spam", "Spam" }
                    option { value: "forward", "Forward" }
                    option { value: "delete", "Delete" }
                }
            }
            if kind() == "forward" {
                input {
                    class: "fwd-input",
                    r#type: "email",
                    placeholder: "forward to… (e.g. me@example.com)",
                    value: "{forward_to}",
                    onchange: move |evt| {
                        let to = evt.value();
                        forward_to.set(to.clone());
                        if !to.trim().is_empty() {
                            on_change.call(RouteAction::from_kind("forward", &to));
                        }
                    },
                }
            }
        }
    }
}

#[component]
fn SignIn() -> Element {
    rsx! {
        section { class: "panel center",
            p { "Sign in with Google to let the router read and clean up your inbox." }
            a { class: "btn primary", href: "/auth/login", "Sign in with Google" }
            p { class: "muted small",
                "Requires a Web-application OAuth client whose redirect URI is "
                code { "http://localhost:8080/auth/callback" }
                " (or your deployed base URL)."
            }
        }
    }
}
