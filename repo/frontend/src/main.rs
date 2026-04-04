use yew::prelude::*;

mod app;
mod router;
mod state;
mod api;
mod components;
mod pages;

fn main() {
    wasm_logger::init(wasm_logger::Config::default());
    yew::Renderer::<app::App>::new().render();
}
