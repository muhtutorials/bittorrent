use axum::{Router, routing::get};
use std::net::SocketAddr;
use tracker::handlers::announce;
use tracker::state::AppState;

const PORT: u16 = 8000;

#[tokio::main]
async fn main() {
    let state = AppState::default();
    let app = Router::new()
        .route("/announce", get(announce::get))
        .with_state(state);
    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{PORT}"))
        .await
        .unwrap();
    println!("server listening on port {PORT}");
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    .unwrap();
}
