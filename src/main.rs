#[cfg(feature = "server")]
fn main() {
    gmail_router::server::main_server();
}

#[cfg(not(feature = "server"))]
fn main() {
    dioxus::launch(gmail_router::App);
}
