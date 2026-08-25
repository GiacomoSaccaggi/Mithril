//! Lazy model manager — loads GGUF on first use, auto-unloads after idle timeout.
//!
//! ```mermaid
//! stateDiagram-v2
//!     [*] --> Unloaded
//!     Unloaded --> Loaded: ensure_loaded
//!     Loaded --> Inferring: infer called
//!     Inferring --> Loaded: done
//!     Loaded --> Unloaded: idle > 60s
//! ```
//!
//! Key design decisions:
//! - `LlamaModel` is `!Send + !Sync` so all inference runs in `std::thread::spawn`
//! - Streaming uses `std::sync::mpsc` bridged to tokio channels
//! - Metal GPU offload (99 layers) on Apple Silicon, CPU on other platforms

#![allow(dead_code)]
use std::num::NonZeroU32;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};
use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::{AddBos, LlamaModel};
use llama_cpp_2::sampling::LlamaSampler;
use parking_lot::Mutex;
use tokio::sync::watch;
use tracing::info;

struct ModelState {
    /// The loaded model. `None` when unloaded.
    model: Option<LlamaModel>,
    /// The backend — kept alive alongside the model.
    /// NOTE: LlamaBackend and LlamaModel are !Send+!Sync, so they are
    /// confined to spawn_blocking tasks and never moved across threads while locked.
    backend: Option<LlamaBackend>,
    last_used: Instant,
}

/// Manages a single GGUF model with lazy loading and automatic unloading.
/// Port of LazyModelManager.kt.
pub struct LazyModelManager {
    model_path: PathBuf,
    unload_after: Duration,
    n_gpu_layers: u32,
    /// Context window size in tokens. Default: 4096. Override via new_with_ctx.
    n_ctx: u32,
    state: Arc<Mutex<ModelState>>,
    shutdown_tx: watch::Sender<bool>,
}

impl LazyModelManager {
    pub fn new(model_path: PathBuf, unload_after_secs: u64) -> Self {
        let n_gpu_layers: u32 = if cfg!(target_os = "macos") && cfg!(target_arch = "aarch64") {
            99 // Metal GPU offload on Apple Silicon
        } else {
            0
        };

        let (shutdown_tx, mut shutdown_rx) = watch::channel(false);

        let state = Arc::new(Mutex::new(ModelState {
            model: None,
            backend: None,
            last_used: Instant::now(),
        }));

        // Spawn background task to auto-unload idle model
        let state_clone = Arc::clone(&state);
        let unload_after = Duration::from_secs(unload_after_secs);
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_secs(10)) => {
                        let mut guard = state_clone.lock();
                        if guard.model.is_some() && guard.last_used.elapsed() > unload_after {
                            info!("LazyModelManager: auto-unloading idle model");
                            guard.model = None;
                            guard.backend = None;
                        }
                    }
                    _ = shutdown_rx.changed() => {
                        if *shutdown_rx.borrow() { break; }
                    }
                }
            }
        });

        Self { model_path, unload_after, n_gpu_layers, n_ctx: 4096, state, shutdown_tx }
    }

    /// Create a manager with a custom context window size.
    pub fn new_with_ctx(model_path: PathBuf, unload_after_secs: u64, n_ctx: u32) -> Self {
        let mut mgr = Self::new(model_path, unload_after_secs);
        mgr.n_ctx = n_ctx;
        mgr
    }

    pub fn is_loaded(&self) -> bool {
        self.state.lock().model.is_some()
    }

    /// Run blocking inference on a thread pool. Port of LazyModelManager.infer().
    pub fn infer(
        &self,
        prompt: &str,
        stop_strings: &[String],
        temperature: f32,
        max_tokens: u32,
    ) -> Result<String> {
        let model_path = self.model_path.clone();
        let n_gpu_layers = self.n_gpu_layers;
        let n_ctx = self.n_ctx;
        let state = Arc::clone(&self.state);
        let prompt = prompt.to_string();
        let stop_strings = stop_strings.to_vec();

        // Run inference in a dedicated thread because LlamaModel+LlamaBatch are !Send
        let result = std::thread::spawn(move || -> Result<String> {
            // Ensure model is loaded
            {
                let mut guard = state.lock();
                if guard.model.is_none() {
                    if !model_path.exists() {
                        return Err(anyhow!(
                            "Model not found at {:?}. Run: mithril download-model",
                            model_path
                        ));
                    }
                    info!("LazyModelManager: loading model {:?}", model_path.file_name());
                    let backend = LlamaBackend::init()?;
                    let params = LlamaModelParams::default().with_n_gpu_layers(n_gpu_layers);
                    let model = LlamaModel::load_from_file(&backend, &model_path, &params)?;
                    guard.backend = Some(backend);
                    guard.model = Some(model);
                    info!("LazyModelManager: model loaded");
                }
                guard.last_used = Instant::now();
            }

            // Hold the lock continuously from load-check through inference start.
            // This prevents two threads from both seeing model.is_none() and loading twice.
            let guard = state.lock();
            let model = guard.model.as_ref()
                .ok_or_else(|| anyhow!("Model not loaded"))?;
            let backend = guard.backend.as_ref()
                .ok_or_else(|| anyhow!("Backend not initialized"))?;

            let ctx_params = LlamaContextParams::default()
                .with_n_ctx(NonZeroU32::new(n_ctx))
                .with_n_batch(n_ctx);
            let mut ctx = model.new_context(backend, ctx_params)?;

            let tokens = model.str_to_token(&prompt, AddBos::Always)?;
            let n_prompt = tokens.len();

            // Decode the initial prompt using add_sequence
            let mut batch = LlamaBatch::new(n_ctx as usize, 1);
            batch.add_sequence(&tokens, 0, false)?;
            ctx.decode(&mut batch)?;

            let mut output = String::new();
            let mut n_cur = n_prompt as i32;

            let mut sampler = LlamaSampler::chain_simple([
                LlamaSampler::temp(temperature),
                LlamaSampler::dist(42),
            ]);

            let mut decoder = encoding_rs::UTF_8.new_decoder();

            loop {
                if n_cur >= max_tokens as i32 + n_prompt as i32 {
                    break;
                }

                let token = sampler.sample(&ctx, -1);

                if model.is_eog_token(token) {
                    break;
                }

                let piece = model
                    .token_to_piece(token, &mut decoder, false, None)
                    .unwrap_or_default();

                output.push_str(&piece);

                let should_stop = stop_strings.iter().any(|s| output.ends_with(s.as_str()));
                if should_stop {
                    for stop in &stop_strings {
                        if output.ends_with(stop.as_str()) {
                            output.truncate(output.len() - stop.len());
                            break;
                        }
                    }
                    break;
                }

                sampler.accept(token);

                // Decode next token
                let mut next_batch = LlamaBatch::new(1, 1);
                next_batch.add(token, n_cur, &[0], true)?;
                ctx.decode(&mut next_batch)?;

                n_cur += 1;
            }

            Ok(output.trim().to_string())
        })
        .join()
        .map_err(|_| anyhow!("Inference thread panicked"))?;

        self.state.lock().last_used = Instant::now();
        result
    }

    /// Run streaming inference. Sends each token piece via `tx` as `Some(piece)`,
    /// then sends `None` when done. The inference thread is detached — the caller
    /// reads from the receiver end and yields chunks to the HTTP response stream.
    pub fn infer_streaming(
        &self,
        prompt: &str,
        stop_strings: &[String],
        temperature: f32,
        max_tokens: u32,
        tx: std::sync::mpsc::SyncSender<Option<String>>,
    ) {
        let model_path = self.model_path.clone();
        let n_gpu_layers = self.n_gpu_layers;
        let n_ctx = self.n_ctx;
        let state = Arc::clone(&self.state);
        let prompt = prompt.to_string();
        let stop_strings = stop_strings.to_vec();
        let state_for_update = Arc::clone(&self.state);

        std::thread::spawn(move || {
            let run = || -> Result<()> {
                {
                    let mut guard = state.lock();
                    if guard.model.is_none() {
                        if !model_path.exists() {
                            return Err(anyhow!("Model not found at {:?}", model_path));
                        }
                        info!("LazyModelManager: loading model {:?}", model_path.file_name());
                        let backend = LlamaBackend::init()?;
                        let params = LlamaModelParams::default().with_n_gpu_layers(n_gpu_layers);
                        let model = LlamaModel::load_from_file(&backend, &model_path, &params)?;
                        guard.backend = Some(backend);
                        guard.model = Some(model);
                        info!("LazyModelManager: model loaded");
                    }
                    guard.last_used = Instant::now();
                }

                let guard = state.lock();
                let model = guard.model.as_ref()
                    .ok_or_else(|| anyhow!("Model not loaded"))?;
                let backend = guard.backend.as_ref()
                    .ok_or_else(|| anyhow!("Backend not initialized"))?;

                let ctx_params = LlamaContextParams::default()
                    .with_n_ctx(NonZeroU32::new(n_ctx));
                let mut ctx = model.new_context(backend, ctx_params)?;

                let tokens = model.str_to_token(&prompt, AddBos::Always)?;
                let n_prompt = tokens.len();

                let mut batch = LlamaBatch::new(n_ctx as usize, 1);
                batch.add_sequence(&tokens, 0, false)?;
                ctx.decode(&mut batch)?;

                let mut accumulated = String::new();
                let mut n_cur = n_prompt as i32;

                let mut sampler = LlamaSampler::chain_simple([
                    LlamaSampler::temp(temperature),
                    LlamaSampler::dist(42),
                ]);
                let mut decoder = encoding_rs::UTF_8.new_decoder();

                loop {
                    if n_cur >= max_tokens as i32 + n_prompt as i32 {
                        break;
                    }
                    let token = sampler.sample(&ctx, -1);
                    if model.is_eog_token(token) {
                        break;
                    }
                    let piece = model
                        .token_to_piece(token, &mut decoder, false, None)
                        .unwrap_or_default();

                    accumulated.push_str(&piece);
                    if stop_strings.iter().any(|s| accumulated.ends_with(s.as_str())) {
                        break;
                    }
                    // Stop generating if receiver dropped (client disconnected) — fix H4
                    if tx.send(Some(piece)).is_err() { break; }
                    sampler.accept(token);

                    let mut next_batch = LlamaBatch::new(1, 1);
                    next_batch.add(token, n_cur, &[0], true)?;
                    ctx.decode(&mut next_batch)?;
                    n_cur += 1;
                }
                Ok(())
            };

            if let Err(e) = run() {
                // Send error marker so the stream reader can surface it
                let _ = tx.send(Some(format!("\n[ERROR: {e}]")));
            }
            // Signal end-of-stream
            let _ = tx.send(None);
            state_for_update.lock().last_used = Instant::now();
        });
    }

    pub fn force_unload(&self) {
        let mut guard = self.state.lock();
        guard.model = None;
        guard.backend = None;
        info!("LazyModelManager: force-unloaded");
    }
}

impl Drop for LazyModelManager {
    fn drop(&mut self) {
        let _ = self.shutdown_tx.send(true);
        self.force_unload();
    }
}
