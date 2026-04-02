use crate::api::StreamEvent;
use crate::auth::{events_ws_url, use_auth};
use leptos::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;

#[derive(Clone, Copy)]
pub struct LiveEventStream {
    pub seq: RwSignal<u64>,
    pub last_event: RwSignal<Option<StreamEvent>>,
    pub connect_count: RwSignal<u64>,
    retry_generation: RwSignal<u64>,
    socket: RwSignal<Option<web_sys::WebSocket>>,
}

impl LiveEventStream {
    pub fn new() -> Self {
        Self {
            seq: RwSignal::new(0),
            last_event: RwSignal::new(None),
            connect_count: RwSignal::new(0),
            retry_generation: RwSignal::new(0),
            socket: RwSignal::new(None),
        }
    }
}

pub fn use_live_event_stream() -> LiveEventStream {
    use_context::<LiveEventStream>()
        .expect("LiveEventStream not in context — wrap with provider in App")
}

fn schedule_ws_retry(retry: RwSignal<u64>, generation: u64) {
    let callback = Closure::<dyn FnMut()>::new(move || {
        if retry.get_untracked() == generation {
            retry.update(|n| *n += 1);
        }
    });

    if let Some(window) = web_sys::window() {
        let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(
            callback.as_ref().unchecked_ref(),
            1000,
        );
    }

    callback.forget();
}

#[component]
pub fn LiveEventStreamBridge() -> impl IntoView {
    let auth = use_auth();
    let stream = use_live_event_stream();

    Effect::new(move |_| {
        let generation = stream.retry_generation.get();
        let token = match auth.token.get() {
            Some(token) => token,
            None => return,
        };

        let has_live_socket = stream.socket.with(|slot| {
            slot.as_ref()
                .map(|ws| {
                    matches!(
                        ws.ready_state(),
                        web_sys::WebSocket::CONNECTING | web_sys::WebSocket::OPEN
                    )
                })
                .unwrap_or(false)
        });
        if has_live_socket {
            return;
        }

        let url = events_ws_url(&token);
        let ws = match web_sys::WebSocket::new(&url) {
            Ok(ws) => ws,
            Err(_) => return,
        };
        stream.socket.set(Some(ws.clone()));

        let connect_count = stream.connect_count;
        let on_open = Closure::<dyn FnMut(web_sys::Event)>::new(move |_| {
            connect_count.update(|count| *count += 1);
        });
        ws.set_onopen(Some(on_open.as_ref().unchecked_ref()));
        on_open.forget();

        let last_event = stream.last_event;
        let seq = stream.seq;
        let on_message =
            Closure::<dyn FnMut(web_sys::MessageEvent)>::new(move |ev: web_sys::MessageEvent| {
                let text = match ev.data().as_string() {
                    Some(text) => text,
                    None => return,
                };
                let event: StreamEvent = match serde_json::from_str(&text) {
                    Ok(event) => event,
                    Err(_) => return,
                };
                last_event.set(Some(event));
                seq.update(|n| *n += 1);
            });
        ws.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
        on_message.forget();

        let socket_on_close = stream.socket;
        let retry_generation = stream.retry_generation;
        let on_close = Closure::<dyn FnMut(web_sys::CloseEvent)>::new(move |_| {
            socket_on_close.set(None);
            schedule_ws_retry(retry_generation, generation);
        });
        ws.set_onclose(Some(on_close.as_ref().unchecked_ref()));
        on_close.forget();

        let socket_on_error = stream.socket;
        let retry_generation = stream.retry_generation;
        let on_error = Closure::<dyn FnMut(web_sys::Event)>::new(move |_| {
            socket_on_error.set(None);
            schedule_ws_retry(retry_generation, generation);
        });
        ws.set_onerror(Some(on_error.as_ref().unchecked_ref()));
        on_error.forget();
    });

    on_cleanup(move || {
        stream.socket.update(|slot| {
            if let Some(ws) = slot.take() {
                ws.set_onopen(None);
                ws.set_onmessage(None);
                ws.set_onclose(None);
                ws.set_onerror(None);
                let _ = ws.close();
            }
        });
    });

    view! {}
}
