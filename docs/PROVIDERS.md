# The Five Istari — Providers

> *"There are five of us. Five wizards."* — Gandalf

Mithril draws power from five sources: Local GGUF models and four cloud providers. Like the Istari sent to Middle-earth, each brings unique strengths.

---

## Provider Overview

| Provider | Type | Best For |
|----------|------|----------|
| **Local (GGUF)** | On-device | Privacy, offline, cost-free |
| **Gemini** | Cloud | Long context, multimodal |
| **OpenAI** | Cloud | GPT-4 quality, broad compatibility |
| **Anthropic** | Cloud | Claude models, thoughtful responses |
| **Groq** | Cloud | Speed, Llama/Mixtral inference |

---

## Provider Configuration

### Setting API Keys

```bash
mithril config set gemini "AIza..."
mithril config set openai "sk-..."
mithril config set anthropic "sk-ant-..."
mithril config set groq "gsk_..."
```

Keys are stored encrypted using Argon2id + AES-256-GCM. See [SECURITY.md](SECURITY.md).

### Configuration File

```yaml
# ~/.mithril/config.yaml
providers:
  default: gemini
  
  local:
    model_path: ~/.mithril/models
    default_model: qwen-7b
    context_length: 4096
    gpu_layers: -1  # -1 = all layers on GPU
    
  gemini:
    model: gemini-2.5-flash
    
  openai:
    model: gpt-4o
    organization: org-xxx  # Optional
    
  anthropic:
    model: claude-3-5-sonnet-20241022
    
  groq:
    model: llama-3.3-70b-versatile
```

---

## Local GGUF Provider

> *"Even the very wise cannot see all ends."*

Run models entirely on your device using llama.cpp.

### Supported Formats

| Format | Support |
|--------|---------|
| GGUF | ✅ Full |
| GGML | ⚠️ Legacy, use GGUF |

### Downloading Models

```bash
# From Hugging Face
mithril download-model TheBloke/Mistral-7B-v0.1-GGUF

# Direct URL
mithril download-model https://example.com/model.gguf

# List downloaded
mithril download-model --list
```

### Model Storage

```
~/.mithril/models/
├── qwen2.5-7b-instruct-q4_k_m.gguf
├── mistral-7b-instruct-v0.2.Q4_K_M.gguf
└── llama-3.2-1b-instruct-q8_0.gguf
```

### Configuration Options

```yaml
providers:
  local:
    model_path: ~/.mithril/models
    default_model: qwen2.5-7b-instruct-q4_k_m
    
    # Context length
    context_length: 4096
    
    # GPU layers (-1 = all, 0 = CPU only)
    gpu_layers: -1
    
    # Batch size for prompt processing
    batch_size: 512
    
    # Threads for CPU inference
    threads: 4
    
    # Keep model in memory between requests
    keep_alive: 300  # seconds
```

### Metal GPU Acceleration

On Apple Silicon, Metal acceleration is automatic when `gpu_layers: -1`.

Check GPU usage:
```bash
# While running inference:
sudo powermetrics --samplers gpu_power
```

### Lazy Loading

Models are loaded on first request and unloaded after idle timeout:

```
Request → Model Loading (3-5s) → Inference → Keep-alive
                                              ↓ (timeout)
                                          Model Unloaded
```

---

## Gemini Provider

> *"Even darkness must pass. A new day will come."*

Google's Gemini models excel at long context and multimodal tasks.

### Available Models

| Model | Context | Best For |
|-------|---------|----------|
| `gemini-2.5-flash` | 1M tokens | Fast, cost-effective |
| `gemini-2.5-pro` | 1M tokens | Complex reasoning |
| `gemini-1.5-pro` | 2M tokens | Maximum context |

### Configuration

```yaml
providers:
  gemini:
    model: gemini-2.5-flash
    
    # API configuration
    api_version: v1
    
    # Safety settings (optional)
    safety_settings:
      harassment: block_medium_and_above
      hate_speech: block_medium_and_above
```

### API Key

Get your key from [Google AI Studio](https://aistudio.google.com/).

```bash
mithril config set gemini "AIzaSy..."
```

---

## OpenAI Provider

> *"All we have to decide is what to do with the time that is given us."*

OpenAI's models offer broad compatibility and proven quality.

### Available Models

| Model | Context | Best For |
|-------|---------|----------|
| `gpt-4o` | 128K | Best quality |
| `gpt-4o-mini` | 128K | Cost-effective |
| `gpt-4-turbo` | 128K | Previous generation |
| `o1` | 200K | Complex reasoning |
| `o1-mini` | 128K | Fast reasoning |

### Configuration

```yaml
providers:
  openai:
    model: gpt-4o
    
    # Optional organization ID
    organization: org-xxx
    
    # Base URL for Azure or compatible APIs
    base_url: https://api.openai.com/v1
```

### API Key

Get your key from [OpenAI Platform](https://platform.openai.com/).

```bash
mithril config set openai "sk-..."
```

### Azure OpenAI

For Azure-hosted models:

```yaml
providers:
  openai:
    base_url: https://your-resource.openai.azure.com
    api_version: "2024-02-15-preview"
    deployment: your-deployment-name
```

---

## Anthropic Provider

> *"A wizard is never late. Nor is he early. He arrives precisely when he means to."*

Anthropic's Claude models are known for thoughtful, nuanced responses.

### Available Models

| Model | Context | Best For |
|-------|---------|----------|
| `claude-sonnet-4-20250514` | 200K | Latest, best quality |
| `claude-3-5-sonnet-20241022` | 200K | Previous generation |
| `claude-3-5-haiku-20241022` | 200K | Fast, cost-effective |
| `claude-3-opus-20240229` | 200K | Most capable |

### Configuration

```yaml
providers:
  anthropic:
    model: claude-sonnet-4-20250514
    
    # Max tokens for response
    max_tokens: 4096
```

### API Key

Get your key from [Anthropic Console](https://console.anthropic.com/).

```bash
mithril config set anthropic "sk-ant-..."
```

---

## Groq Provider

> *"Not all those who wander are lost."*

Groq provides blazing-fast inference for open-source models.

### Available Models

| Model | Context | Best For |
|-------|---------|----------|
| `llama-3.3-70b-versatile` | 128K | Best quality |
| `llama-3.1-8b-instant` | 128K | Fast, efficient |
| `mixtral-8x7b-32768` | 32K | Balanced |
| `gemma2-9b-it` | 8K | Compact |

### Configuration

```yaml
providers:
  groq:
    model: llama-3.3-70b-versatile
```

### API Key

Get your key from [Groq Console](https://console.groq.com/).

```bash
mithril config set groq "gsk_..."
```

---

## Provider Selection

### Per-Request

Specify provider in fellowship config:

```yaml
agents:
  - name: worker
    provider: gemini
    model: gemini-2.5-flash
    
  - name: reviewer
    provider: anthropic
    model: claude-sonnet-4-20250514
```

### CLI Override

```bash
mithril chat --model gemini/gemini-2.5-pro
mithril forge "hello" --model openai/gpt-4o
```

### @Mentions

```
@gguf summarize this using the local model
```

---

## Retry and Resilience

> *"Despair is only for those who see the end beyond all doubt."*

All cloud providers benefit from automatic retry with exponential backoff.

### Retry Configuration

```yaml
providers:
  retry:
    max_attempts: 5
    initial_delay_ms: 1000
    max_delay_ms: 30000
    multiplier: 2.0
    jitter: 0.25
```

### Retry Schedule

| Attempt | Base Delay | With Jitter |
|---------|------------|-------------|
| 1 | 0s | 0s |
| 2 | 1s | 0.75-1.25s |
| 3 | 2s | 1.5-2.5s |
| 4 | 4s | 3-5s |
| 5 | 8s | 6-10s |

### Retryable Errors

| Error | Retried |
|-------|---------|
| Network timeout | ✅ |
| 429 Rate limit | ✅ |
| 500 Server error | ✅ |
| 502/503/504 | ✅ |
| 401 Auth error | ❌ |
| 400 Bad request | ❌ |

### Provider Fallback

Configure fallback chain:

```yaml
providers:
  fallback:
    - gemini
    - openai
    - groq
```

If primary fails after all retries, next provider is tried.

---

## Token Tracking

Each provider tracks token usage:

```
/tokens
```

Output:
```
Token Usage (this session):
  gemini-2.5-flash:   Input: 3,500  Output: 8,200
  local/qwen-7b:      Input: 1,200  Output: 2,100
  
Total: 15,000 tokens
```

### Per-Agent Budgets

```yaml
agents:
  - name: worker
    provider: gemini
    token_budget: 50000
    
  - name: reviewer
    provider: anthropic
    token_budget: 20000
```

---

## Provider Comparison

| Feature | Local | Gemini | OpenAI | Anthropic | Groq |
|---------|-------|--------|--------|-----------|------|
| Cost | Free | $$$ | $$$$ | $$$$ | $$ |
| Speed | Varies | Fast | Medium | Medium | Fastest |
| Privacy | ✅ Best | Cloud | Cloud | Cloud | Cloud |
| Context | 4-32K | 1-2M | 128K | 200K | 8-128K |
| Quality | Varies | Excellent | Excellent | Excellent | Good |
| Offline | ✅ Yes | ❌ No | ❌ No | ❌ No | ❌ No |

---

> *"Many that live deserve death. And some that die deserve life. Can you give it to them?"*
