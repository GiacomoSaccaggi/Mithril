# Mithril: Jupyter, GGUF Models, & Memory Optimization Guide

This guide covers everything you need to know about installing Mithril in Jupyter/Linux environments, managing local GGUF models, memory and VRAM planning for different quantizations, and orchestrating hybrid multi-agent workflows.

---

## 1. Installation

Mithril is packaged with zero dynamic OpenSSL dependencies and fully static/portable binaries.

### Option A: Via `pip` (Recommended for Jupyter / Conda)
Run directly inside your terminal or within a Jupyter notebook code cell:

```bash
pip install --upgrade mithril-cli
mithril --version
```

In a Jupyter Notebook cell:
```python
!pip install --upgrade mithril-cli
!mithril --version
```

### Option B: Standalone Shell Installer
```bash
curl -fsSL https://raw.githubusercontent.com/GiacomoSaccaggi/mithril/main/install.sh | bash
```

The installer automatically detects your active shell (`~/.bashrc`, `~/.zshrc`, or `~/.profile`) and configures `PATH` if necessary.

---

## 2. GGUF Model Management

Mithril embeds native `llama.cpp` for local inference without needing external runtimes.

### Built-in Model Catalog
View all pre-configured, tested models:
```bash
mithril download-model --list
```

Preconfigured models include:
- `qwen-1.5b` (Qwen 2.5 Coder 1.5B) — ~1.2 GB (Ultra-fast, ideal for low-RAM machines or lightweight triage)
- `qwen-7b` (Qwen 2.5 Coder 7B) — ~4.5 GB
- `qwen-14b` (Qwen 2.5 Coder 14B) — ~9.0 GB (Strong coding capabilities)
- `llama-8b` (Llama 3.1 8B Instruct) — ~5.0 GB
- `deepseek-6.7b` (DeepSeek Coder 6.7B) — ~4.5 GB
- `phi-3.5` (Phi-3.5 Mini 3.8B) — ~2.5 GB

### Downloading Models
Download any model from the catalog with:
```bash
mithril download-model -m qwen-1.5b
```
Models are stored in `~/.mithril/models/`.

### Using Custom GGUF Quantizations from Hugging Face
You can use any custom `.gguf` file (e.g. from Hugging Face repositories):
1. Download the `.gguf` file.
2. Place it in `~/.mithril/models/`:
   ```bash
   mkdir -p ~/.mithril/models
   cp /path/to/your-model-q5_k_m.gguf ~/.mithril/models/
   ```
3. Reference the filename or model ID in Mithril CLI or config files.

---

## 3. Local Inference & Server Setup in Jupyter

### One-Shot Inference
Test local inference instantly from the command line:
```bash
mithril forge "Write a Python function to calculate the Fibonacci sequence"
```

### Starting the Background Server in Jupyter
Start Mithril's Ollama and OpenAI-compatible server on port `16180`:

```python
import subprocess
import time
import requests

# Start Mithril server in background
server_process = subprocess.Popen(
    ["mithril", "serve", "--port", "16180"],
    stdout=subprocess.PIPE,
    stderr=subprocess.PIPE
)

# Wait 2 seconds for initialization
time.sleep(2)

# Verify server status
res = requests.get("http://localhost:16180/api/version")
print("Mithril Server:", res.json())
```

### Consuming the Server in Python

#### Using the OpenAI Python SDK:
```python
from openai import OpenAI

client = OpenAI(
    base_url="http://localhost:16180/v1",
    api_key="mithril"  # Any non-empty string
)

response = client.chat.completions.create(
    model="qwen-1.5b",
    messages=[
        {"role": "system", "content": "You are an expert data science assistant."},
        {"role": "user", "content": "How do I normalize a matrix with NumPy?"}
    ]
)

print(response.choices[0].message.content)
```

#### Using Direct HTTP Requests (Ollama API):
```python
import requests

payload = {
    "model": "qwen-1.5b",
    "prompt": "Explain gradient descent in 3 bullet points.",
    "stream": False
}

response = requests.post("http://localhost:16180/api/generate", json=payload)
print(response.json().get("response"))
```

---

## 4. GGUF Quantization, Memory (RAM / VRAM), & Context Planning

When choosing GGUF models and quantizations, memory requirements depend on model weights plus the active context window (KV cache).

### Memory Breakdown for Qwen 2.5 Coder 14B

| Quantization | File Size | Min RAM/VRAM (4k Context) | Recommended RAM/VRAM (32k Context) | Accuracy & Characteristics |
| :--- | :--- | :--- | :--- | :--- |
| **Q2_K** | ~5.5 GB | ~8 GB | ~10–12 GB | Ultra-lightweight; significant loss in complex syntax & reasoning. |
| **Q4_K_M** *(Default)* | ~9.0 GB | ~12 GB | ~16 GB | **Optimal balance**: retains ~98% FP16 accuracy, fits within standard RAM/VRAM. |
| **Q5_K_M** | ~10.5 GB | ~14 GB | ~16–20 GB | Higher fidelity on intricate code syntax & subtleties. |
| **Q8_0** | ~15.0 GB | ~18 GB | ~20–24 GB | Maximum fidelity; virtually identical to unquantized FP16. |

### Context Window (KV Cache) Memory Impact
- **Base Weights**: Loaded into memory once upon initialization.
- **KV Cache Memory Scaling**:
  - `4k–8k context`: Adds ~1.0–1.5 GB memory.
  - `16k–32k context`: Adds ~3.0–5.5 GB memory.
- In Mithril, context size defaults to 4096 tokens and can be customized via API options (`num_predict`, `temperature`) or engine configuration.

### Hardware Acceleration & Compute Offloading
- **Apple Silicon (macOS)**: Mithril automatically sets `n_gpu_layers = 99`, offloading all layers directly into unified Metal GPU memory.
- **Linux / Cloud VMs / JupyterHub**: Mithril executes optimized multi-threaded SIMD on CPU (AVX2, AVX-512, or ARM NEON). If physical RAM is constrained, `mmap` loads active pages dynamically to prevent OOM kernel termination.

---

## 5. Memory Strategy: Hybrid Fellowship Architecture

For memory-constrained environments (e.g. 8 GB–12 GB total RAM), the most efficient setup is Mithril's **Hybrid Fellowship**:
1. Run a lightweight local model (`qwen-1.5b` requiring only ~1.2 GB RAM) as the **controller/router**.
2. Delegate deep reasoning, heavy refactoring, or large-context queries to cloud models (Gemini, Claude, OpenAI, Groq) or a dedicated GPU server.

### Example Configuration (`.mithril/fellowship.yaml`):

```yaml
name: "jupyter-fellowship"

controller:
  provider: local
  model: qwen-1.5b       # ~1.2 GB RAM for fast local intent routing
  context_window: 4

agents:
  - name: cloud-reasoning
    provider: gemini
    model: gemini-2.0-flash
    api_key_env: GEMINI_API_KEY
    role: "Deep code analysis, complex data transformations, and large context tasks"

  - name: local-coder
    provider: local
    model: qwen-1.5b
    role: "Fast boilerplate, docstrings, and quick unit tests"
```

Start interactive chat with this fellowship:
```bash
mithril chat --fellowship jupyter-fellowship
```
