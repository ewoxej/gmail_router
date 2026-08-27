use dioxus::prelude::*;

pub mod api;
pub mod models;
pub mod routes;
pub mod ui;

#[cfg(feature = "server")]
pub mod auth;
#[cfg(feature = "server")]
pub mod config;
#[cfg(feature = "server")]
pub mod db;
#[cfg(feature = "server")]
pub mod decision;
#[cfg(feature = "server")]
pub mod engine;
#[cfg(feature = "server")]
pub mod gmail;
#[cfg(feature = "server")]
pub mod migrate;
#[cfg(feature = "server")]
pub mod processor;
#[cfg(feature = "server")]
pub mod server;

pub use routes::Route;

#[component]
pub fn App() -> Element {
    rsx! {
        Router::<Route> {}
    }
}
