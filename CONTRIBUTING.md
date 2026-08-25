# Contributing to Mithril

This guide covers how to add new providers, tools, and models to Mithril.

---

## Adding a new provider

A provider is a cloud (or local) LLM backend. It implements the `ChatProvider` trait.

### Step 1 — Create the provider file

Create `src/providers/myservice.rs`:

```rust
use super::{ChatMessage, ChatProvider, StreamChunk, ToolCallResult, ToolDefinition};
use anyhow::Result;
use async_trait::async_trait;
use futures::StreamExt;

pub struct MyServiceProvider {
    api_key: String,
    model: String,
    client: reqwest::Client,
}

impl MyServiceProvider {
    pub fn new(api_key: String, model: &str) -> Self {
        Self {
            api_key,
            model: model.to_string(),
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait]
impl ChatProvider for MyServiceProvider {
    fn name(&self) -> &str { "myservice" }
    fn model(&self) -> &str { &self.model }

    async fn chat(&self, messages: &[ChatMessage]) -> Result<String> {
        // Build request, call API, return text response
        todo!()
    }

    async fn chat_stream(
        &self,
        messages: &[ChatMessage],
        on_chunk: Box<dyn Fn(StreamChunk) + Send>,
    ) -> Result<String> {
        // Call streaming endpoint, parse SSE, call on_chunk per token
        // See gemini.rs or anthropic.rs for reference implementations
        todo!()
    }

    async fn is_available(&self) -> bool {
        !self.api_key.is_empty()
    }

    // Optional: implement tool calling
    async fn chat_with_tools(
        &self,
        messages: &[ChatMessage],
        tools: &[ToolDefinition],
    ) -> Result<ToolCallResult> {
        // If your API supports function calling, implement here
        // See openai.rs for reference
        // Default: falls back to plain chat (already provided by trait default)
        let text = self.chat(messages).await?;
        Ok(ToolCallResult::Text(text))
    }
}
```

### Step 2 — Export from `mod.rs`

In `src/providers/mod.rs`:

```rust
mod myservice;
pub use myservice::MyServiceProvider;
```

### Step 3 — Add to factory

In `src/providers/mod.rs`, in `create_provider()`:

```rust
"myservice" => {
    let api_key = config
        .get_credential("myservice")?
        .ok_or_else(|| anyhow::anyhow!(
            "MyService API key not configured. Run: mithril config set myservice <key>"
        ))?;
    Ok(Box::new(MyServiceProvider::new(api_key, &config.providers.myservice.model)))
}
```

### Step 4 — Add settings struct

In `src/config/mod.rs`:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MyServiceSettings {
    #[serde(default = "default_myservice_model")]
    pub model: String,
}
impl Default for MyServiceSettings {
    fn default() -> Self { Self { model: default_myservice_model() } }
}
fn default_myservice_model() -> String { "myservice-model-v1".to_string() }
```

Add to `ProviderSettings`:

```rust
pub struct ProviderSettings {
    pub gemini: GeminiSettings,
    pub openai: OpenAISettings,
    pub anthropic: AnthropicSettings,
    pub myservice: MyServiceSettings,  // add this
}
```

### Step 5 — Add to CLI

In `src/cli/chat.rs`, `print_providers()` lists all providers. Add yours to `providers::available_providers()` in `src/providers/mod.rs`:

```rust
pub fn available_providers() -> Vec<&'static str> {
    vec!["local", "gemini", "openai", "anthropic", "myservice"]
}
```

### Step 6 — Document

Add your provider to `docs/PROVIDERS.md` and the compatibility matrix in `docs/COMPATIBILITY.md`.

### Checklist

- [ ] `src/providers/myservice.rs` implements `ChatProvider`
- [ ] Exported from `src/providers/mod.rs`
- [ ] Added to `create_provider()` factory
- [ ] Settings struct in `src/config/mod.rs`
- [ ] Added to `available_providers()`
- [ ] Added to `docs/PROVIDERS.md`

---

## Adding a new tool

Tools are exposed via MCP `tools/call` and executed locally. There are 21 built-in tools; new ones can be added without modifying existing code.

### Step 1 — Implement the `Tool` trait

In `src/tools/implementations.rs`, add a new struct:

```rust
pub struct MyTool {
    // Add operators or state your tool needs
    file_op: FileOperator,
}

impl MyTool {
    pub fn new(op: FileOperator) -> Self { Self { file_op: op } }
}

impl Tool for MyTool {
    fn name(&self) -> &'static str { "my_tool" }

    fn description(&self) -> &'static str {
        "Brief description of what this tool does"
    }

    fn parameters(&self) -> Vec<ToolParam> {
        vec![
            p("target", "The file path to operate on", true),
            p("mode", "Optional mode: 'fast' or 'thorough'", false),
        ]
    }

    fn execute(&self, args: &HashMap<String, String>) -> ToolResult {
        let target = match args.get("target") {
            Some(v) => v,
            None => return ToolResult::err("Missing 'target'"),
        };
        // Implement your logic here
        let result = self.file_op.read_file(target);
        ToolResult::ok(result)
    }
}
```

> **Note:** `execute()` is synchronous. If you need async (HTTP calls, terminal), use the `block_on_async` helper already defined in `implementations.rs`.

### Step 2 — Register in the factory

In `src/tools/mod.rs`, add your tool to `create_default_registry()`:

```rust
pub fn create_default_registry(base_path: &str) -> ToolRegistry {
    // ... existing tools ...
    registry.register(MyTool::new(file_op.clone()));
    registry
}
```

### Step 3 — Document

Add your tool to `docs/TOOLS.md`.

### Checklist

- [ ] Implements `Tool` trait in `implementations.rs`
- [ ] Registered in `create_default_registry()`
- [ ] `name()` is lowercase with underscores (e.g., `my_tool`)
- [ ] `description()` is clear enough for an LLM to understand when to use it
- [ ] All required parameters have `required: true`
- [ ] Returns `ToolResult::err()` with a descriptive message on failure
- [ ] Added to `docs/TOOLS.md`

---

## Adding a new model

Models are compile-time constants in `src/engine/model_catalog.rs`.

### Step 1 — Add to `MODELS`

```rust
pub const MODELS: &[ModelInfo] = &[
    // ... existing models ...
    ModelInfo {
        id: "my-model-3b",
        display_name: "My Model 3B (Description, ~2GB)",
        file_name: "my-model-3b-instruct-q4_k_m.gguf",
        download_url: "https://huggingface.co/org/repo/resolve/main/my-model-3b.gguf",
        family: "my-model",
        parameter_size: "3B",
        quantization: "Q4_K_M",
        chat_template: ChatTemplate::ChatML,  // pick the right template
    },
];
```

### Step 2 — Add Ollama-style aliases (optional)

In `normalize_ollama_name()`:

```rust
fn normalize_ollama_name(name: &str) -> &'static str {
    match name {
        // ... existing aliases ...
        "my-model:3b" | "my-model-3b" => "my-model-3b",
        _ => "",
    }
}
```

### Step 3 — Choose the right chat template

| Template | Use when |
|----------|----------|
| `ChatML` | Model uses `<\|im_start\|>` / `<\|im_end\|>` |
| `Llama3` | Model uses `<\|start_header_id\|>` / `<\|eot_id\|>` |
| `Phi3` | Model uses `<\|user\|>` / `<\|end\|>` |

Check the model card on HuggingFace or the GGUF metadata for the correct format.

### Step 4 — Test

```bash
mithril download-model --model my-model-3b
mithril forge "Hello!"
```

### Checklist

- [ ] Added to `MODELS` constant
- [ ] Correct `chat_template` selected
- [ ] Valid HuggingFace download URL
- [ ] Ollama-style aliases added (optional but helpful)
- [ ] Added to `README.md` model table
- [ ] Added to `docs/CLI.md` download reference

---

## Adding a new chat template

If a model uses a format not covered by `ChatML`, `Llama3`, or `Phi3`:

### Step 1 — Add enum variant

In `src/engine/chat_template.rs`:

```rust
pub enum ChatTemplate {
    ChatML,
    Llama3,
    Phi3,
    MyFormat,  // add this
}
```

### Step 2 — Implement formatting

```rust
pub fn format_chat(template: ChatTemplate, messages: &[ChatMessage]) -> String {
    match template {
        ChatTemplate::ChatML => format_chatml(messages),
        ChatTemplate::Llama3 => format_llama3(messages),
        ChatTemplate::Phi3 => format_phi3(messages),
        ChatTemplate::MyFormat => format_my_format(messages),
    }
}

fn format_my_format(messages: &[ChatMessage]) -> String {
    let mut s = String::new();
    for msg in messages {
        s.push_str(&format!("[{}]\n{}\n[/{}]\n", msg.role, msg.content, msg.role));
    }
    s.push_str("[assistant]\n");
    s
}
```

### Step 3 — Add stop tokens

```rust
pub fn get_stop_tokens(template: ChatTemplate) -> Vec<String> {
    match template {
        // ...
        ChatTemplate::MyFormat => vec!["[/assistant]".into(), "[user]".into()],
    }
}
```

### Step 4 — Write tests

```rust
#[test]
fn test_my_format() {
    let messages = vec![ChatMessage::new("user", "Hi")];
    let result = format_chat(ChatTemplate::MyFormat, &messages);
    assert!(result.contains("[user]\nHi\n[/user]\n"));
    assert!(result.ends_with("[assistant]\n"));
}
```

---

## Code style

- Match the existing style: no unnecessary comments, minimal code, no unused imports
- All new public functions need doc comments (`///`)
- New modules must be added to `src/lib.rs` as `pub mod`
- Run `cargo check` before submitting — warnings are acceptable, errors are not
- Run `cargo test --lib` — all existing tests must pass

## Running tests

```bash
# Unit tests only (fast, no model needed)
cargo test --lib

# All tests including integration (requires model file for full coverage)
cargo test

# Single test
cargo test test_encrypt_decrypt_roundtrip
```

---

## Adding a new frontend (like Telegram)

A "frontend" is anything that claims a `SharedSession` and exchanges messages with an LLM.

### Step 1 — Register a frontend ID

In `src/session/mod.rs`, add a constant:

```rust
pub const FRONTEND_MYAPP: u8 = 3;  // next available ID
```

And the name mapping:

```rust
pub fn frontend_name(id: u8) -> &'static str {
    match id {
        FRONTEND_TERMINAL => "terminal",
        FRONTEND_TELEGRAM => "telegram",
        FRONTEND_JUNIE    => "junie",
        FRONTEND_MYAPP    => "myapp",   // add this
        _ => "none",
    }
}
```

### Step 2 — Create the frontend module

Create `src/cli/myapp.rs`. Minimal structure:

```rust
use crate::session::{SharedSession, FRONTEND_MYAPP};
use tokio_util::sync::CancellationToken;

pub async fn run_with_session(session: SharedSession, cancel: CancellationToken) -> anyhow::Result<()> {
    // 1. Claim the frontend
    session.claim_frontend(FRONTEND_MYAPP)?;

    // 2. Your event loop
    loop {
        tokio::select! {
            // receive input from your source
            Some(user_text) = receive_message() => {
                session.push(ChatMessage::user(&user_text));
                let snap = session.snapshot();
                let config = MithrilConfig::load()?;
                let provider = providers::create_provider(&session.provider_name, &config)?;
                let response = provider.chat(&snap).await?;
                session.push(ChatMessage::assistant(&response));
                send_response(&response).await;
            }
            _ = cancel.cancelled() => break,
        }
    }

    // 3. Release the frontend
    session.release_frontend(FRONTEND_MYAPP);
    Ok(())
}
```

### Step 3 — Add CLI subcommand

In `src/main.rs`:

```rust
/// Start the MyApp frontend
MyApp {
    #[arg(long)]
    session: Option<String>,
},
```

And dispatch:

```rust
Commands::MyApp { session } => cli::myapp::run(session.as_deref()).await,
```

### Step 4 — Add `/start-myapp` command to chat

In `src/cli/chat.rs`, in `handle_command()`:

```rust
"/start-myapp" => {
    session.release_frontend(FRONTEND_TERMINAL);
    // launch your frontend
    CommandResult::Continue
}
```

### Step 5 — Export from `cli/mod.rs`

```rust
pub mod myapp;
```

### Checklist

- [ ] Frontend ID constant in `session/mod.rs`
- [ ] `frontend_name()` updated
- [ ] `src/cli/myapp.rs` with `run_with_session()`
- [ ] CLI subcommand in `main.rs`
- [ ] `/start-myapp` in `chat.rs`
- [ ] Exported from `cli/mod.rs`
- [ ] Added to `docs/SESSION.md` and `docs/COMPATIBILITY.md`
