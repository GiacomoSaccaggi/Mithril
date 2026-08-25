#![allow(dead_code)]
use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use axum::{extract::State, routing::{get, post}, Json, Router};
use parking_lot::Mutex;
use serde_json::json;
use tokio::net::TcpListener;
use tower::limit::ConcurrencyLimitLayer;
use tower_http::cors::CorsLayer;
use tracing::info;

use crate::engine::lazy_model::LazyModelManager;
use crate::engine::model_catalog::MODELS;
use crate::operators::{file::FileOperator, scan::ScanOperator};
use crate::tools::{self, registry::ToolRegistry};

#[derive(Clone)]
pub struct AppState {
    pub model_manager: Arc<LazyModelManager>,
    pub tool_registry: Arc<ToolRegistry>,
    pub file_operator: Arc<FileOperator>,
    pub scan_operator: Arc<ScanOperator>,
    pub project_path: String,
    /// Tracks active model downloads: model_id -> status
    pub active_downloads: Arc<Mutex<HashMap<String, String>>>,
    /// Optional bearer token required for inference/MCP routes (None = no auth)
    pub api_token: Option<String>,
    /// Fired on SIGINT — handlers should check this to abort long operations
    pub shutdown: tokio_util::sync::CancellationToken,
    /// Optional flow config — when set, /api/chat routes through the FlowRunner
    /// instead of directly calling a provider. Loaded from .mithril-flow.yaml.
    pub flow_config: Option<crate::flow::FlowConfig>,
}

pub struct MithrilServer;

impl MithrilServer {
    pub async fn start(port: u16, project_path: &str) -> Result<()> {
        let model_dir = dirs::home_dir()
            .unwrap_or_default()
            .join(".mithril/models");

        let model_path = model_dir.join(MODELS[0].file_name);
        let model_manager = Arc::new(LazyModelManager::new(model_path, 60));

        let api_token = crate::config::MithrilConfig::load()
            .ok()
            .and_then(|c| c.api_token);

        if api_token.is_some() {
            info!("API token authentication enabled for inference/MCP routes");
        }

        // Load flow config ONLY if an explicit file exists — never activate flow mode
        // from the built-in default, to avoid confusing standard API clients.
        let flow_config = {
            let local = std::path::Path::new(project_path).join(".mithril-flow.yaml");
            let global = dirs::home_dir().unwrap_or_default().join(".mithril/flows/default.yaml");
            if local.exists() {
                let cfg = crate::flow::FlowConfig::load_from(&local).ok();
                if cfg.is_some() { info!("Flow mode enabled from {} — /api/chat routes through FlowRunner", local.display()); }
                cfg
            } else if global.exists() {
                let cfg = crate::flow::FlowConfig::load_from(&global).ok();
                if cfg.is_some() { info!("Flow mode enabled from {} — /api/chat routes through FlowRunner", global.display()); }
                cfg
            } else {
                info!("No flow config found — /api/chat uses direct provider mode");
                None
            }
        };

        let state = AppState {
            model_manager: Arc::clone(&model_manager),
            tool_registry: Arc::new(tools::create_default_registry(project_path)),
            file_operator: Arc::new(FileOperator::new(project_path)),
            scan_operator: Arc::new(ScanOperator::new(project_path)),
            project_path: project_path.to_string(),
            active_downloads: Arc::new(Mutex::new(HashMap::new())),
            api_token,
            shutdown: tokio_util::sync::CancellationToken::new(),
            flow_config,
        };

        // M1: health/version NOT rate-limited — orchestrators must always be able to probe them
        // M3: with_state() applied to the merged router AFTER merge — Axum 0.7 propagates
        //     state to all sub-routers correctly. ConcurrencyLimitLayer on inference_routes
        //     is applied before merge which is the correct ordering for tower layers.
        let public_routes = Router::new()
            .route("/health", get(health))
            .route("/api/tags", get(super::ollama::list_models))
            .route("/api/version", get(super::ollama::version))
            .route("/api/ps", get(super::ollama::running_models))
            .route("/api/show", post(super::ollama::show_model))
            .route("/v1/models", get(super::openai::list_models));

        // Inference + MCP routes: rate-limited to prevent resource exhaustion
        let inference_routes = Router::new()
            .route("/api/generate", post(super::ollama::generate))
            .route("/api/chat", post(super::ollama::chat))
            .route("/api/pull", post(super::ollama::pull_model))
            .route("/api/embed", post(super::ollama::embed))
            .route("/v1/chat/completions", post(super::openai::chat_completions))
            .route("/mcp", post(super::mcp::handle_mcp))
            .layer(ConcurrencyLimitLayer::new(10));

        // Clone shutdown token BEFORE state is consumed by with_state()
        let shutdown_token = state.shutdown.clone();
        let model_manager_for_shutdown = Arc::clone(&model_manager);

        let app = public_routes
            .merge(inference_routes)
            .layer({
                // CORS: permissive in dev, configurable for production
                // TODO: read allowed_origins from MithrilConfig when deploying publicly
                CorsLayer::permissive()
            })
            .with_state(state);

        let listener = TcpListener::bind(format!("0.0.0.0:{port}")).await?;
        info!("Mithril listening on port {port}");
        // H4: axum's with_graceful_shutdown waits for all connections to close
        // BEFORE the future resolves. So: signal shutdown, let axum drain naturally,
        // then unload model only after the serve future returns.
        axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                tokio::signal::ctrl_c()
                    .await
                    .expect("failed to install Ctrl+C handler");
                info!("Shutdown signal received — draining connections...");
                shutdown_token.cancel(); // signal handlers to abort new work
            })
            .await?;
        // Model unloaded AFTER axum has drained all connections
        info!("Connections drained. Unloading model...");
        model_manager_for_shutdown.force_unload();

        Ok(())
    }
}

/// Build the axum Router — extracted for testability.
pub fn build_app(state: AppState) -> axum::Router {
    let public_routes = axum::Router::new()
        .route("/health", axum::routing::get(health))
        .route("/api/tags", axum::routing::get(super::ollama::list_models))
        .route("/api/version", axum::routing::get(super::ollama::version))
        .route("/api/ps", axum::routing::get(super::ollama::running_models))
        .route("/api/show", axum::routing::post(super::ollama::show_model))
        .route("/v1/models", axum::routing::get(super::openai::list_models));

    let inference_routes = axum::Router::new()
        .route("/api/generate", axum::routing::post(super::ollama::generate))
        .route("/api/chat", axum::routing::post(super::ollama::chat))
        .route("/api/pull", axum::routing::post(super::ollama::pull_model))
        .route("/api/embed", axum::routing::post(super::ollama::embed))
        .route("/v1/chat/completions", axum::routing::post(super::openai::chat_completions))
        .route("/mcp", axum::routing::post(super::mcp::handle_mcp))
        .layer(ConcurrencyLimitLayer::new(10));

    public_routes
        .merge(inference_routes)
        .layer(CorsLayer::permissive())
        .with_state(state)
}

async fn health(State(state): State<AppState>) -> Json<serde_json::Value> {
    Json(json!({
        "status": "ok",
        "model_loaded": state.model_manager.is_loaded(),
        "version": "0.1.0"
    }))
}

/// Public re-export for integration tests
pub async fn server_health_handler(
    state: axum::extract::State<AppState>,
) -> Json<serde_json::Value> {
    health(state).await
}
