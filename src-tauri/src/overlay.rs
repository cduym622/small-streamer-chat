//! A small local web server for the Overlay. OBS (or any browser) loads the
//! Overlay page from it, and the page receives Practice Chat messages over
//! Server-Sent Events. It listens on 127.0.0.1 only, so it isn't reachable
//! from other machines.

use std::convert::Infallible;

use axum::{
    Router,
    extract::State,
    http::{StatusCode, Uri, header},
    response::{
        IntoResponse, Redirect, Response,
        sse::{Event, KeepAlive, Sse},
    },
    routing::get,
};
use tauri::AppHandle;
use tokio::net::TcpListener;
use tokio_stream::{Stream, StreamExt, wrappers::BroadcastStream};

use crate::engine::EngineHandle;

pub const PORT: u16 = 37319;

pub fn url() -> String {
    format!("http://127.0.0.1:{PORT}/")
}

#[derive(Clone)]
struct ServerState {
    app: AppHandle,
    engine: EngineHandle,
}

pub fn spawn(app: AppHandle, engine: EngineHandle) {
    tauri::async_runtime::spawn(async move {
        if let Err(error) = serve(app.clone(), engine).await {
            crate::problems::report(
                &app,
                format!("The Overlay couldn't start on port {PORT}: {error}"),
            );
        }
    });
}

async fn serve(app: AppHandle, engine: EngineHandle) -> std::io::Result<()> {
    let router = Router::new()
        .route("/events", get(events))
        .fallback(get(page_asset))
        .with_state(ServerState { app, engine });
    let listener = TcpListener::bind(("127.0.0.1", PORT)).await?;
    axum::serve(listener, router).await
}

/// Streams Practice Chat messages to the Overlay page.
async fn events(State(state): State<ServerState>) -> Response {
    let stream: std::pin::Pin<Box<dyn Stream<Item = Result<Event, Infallible>> + Send>> = Box::pin(
        BroadcastStream::new(state.engine.subscribe())
            // A slow Overlay that misses messages just skips them.
            .filter_map(|message| message.ok())
            .filter_map(|message| {
                Event::default()
                    .event("chat-message")
                    .json_data(message)
                    .ok()
            })
            .map(Ok),
    );
    let sse = Sse::new(stream).keep_alive(KeepAlive::default());

    // In dev the page is served by Vite on another origin.
    if tauri::is_dev() {
        ([(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")], sse).into_response()
    } else {
        sse.into_response()
    }
}

/// Serves the Overlay page and its built assets from the app's embedded
/// frontend. Nothing else from the frontend is exposed.
async fn page_asset(State(state): State<ServerState>, uri: Uri) -> Response {
    let path = match uri.path() {
        "/" => "/overlay.html",
        path => path,
    };
    if path != "/overlay.html" && !path.starts_with("/assets/") {
        return StatusCode::NOT_FOUND.into_response();
    }

    // In dev, hand off to the Vite dev server so the Overlay hot-reloads.
    if tauri::is_dev()
        && path == "/overlay.html"
        && let Some(dev_url) = &state.app.config().build.dev_url
    {
        let target = format!("{}overlay.html?server=http://127.0.0.1:{PORT}", dev_url);
        return Redirect::temporary(&target).into_response();
    }

    match state.app.asset_resolver().get(path.to_string()) {
        Some(asset) => ([(header::CONTENT_TYPE, asset.mime_type)], asset.bytes).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}
