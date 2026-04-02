mod api;
mod app;
mod auth;
mod models;
mod pages;

use wasm_bindgen::prelude::wasm_bindgen;

#[wasm_bindgen(start)]
pub fn main() {
    console_error_panic_hook::set_once();
    leptos::mount::mount_to_body(app::App);
}
