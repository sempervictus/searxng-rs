//! Route definitions

use super::handlers;
use super::state::AppState;
use axum::{routing::get, Router};
use tower_http::cors::{Any, CorsLayer};

/// Create the application router with all routes
pub fn create_router(state: AppState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    Router::new()
        // Main routes
        .route("/", get(handlers::index))
        .route("/search", get(handlers::search))
        .route("/about", get(handlers::about))
        .route("/usage", get(handlers::usage))
        .route(
            "/preferences",
            get(handlers::preferences).post(handlers::preferences),
        )
        .route("/stats", get(handlers::stats))
        // API routes
        .route("/health", get(handlers::health))
        .route("/config", get(handlers::config))
        .route("/autocomplete", get(handlers::autocomplete))
        // MCP route
        .route("/mcp", get(handlers::mcp_post))
        // Static routes
        .route("/robots.txt", get(handlers::robots_txt))
        .route("/favicon.ico", get(handlers::favicon))
        // Add middleware
        .layer(cors)
        // Add state
        .with_state(state)
}
