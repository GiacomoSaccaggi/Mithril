# Flow — The Orchestration Council

Multi-agent orchestration: classify requests, route to agents, manage tool execution, handle delegation.

## Key Concept

A fast controller (local GGUF model like `jevos-v4` or `qwen-1.5b`, or JEV System 1 provider) classifies every request in zero-shot fashion (free or ultra-low latency), then routes to the right agent. Agents can delegate to each other via NEXT/TASK protocol.

## How It Works

1. **Classification** — Fast decision model reads user message, outputs agent name (~20–50ms via JEV / local GGUF)
2. **Agent Loop** — Selected agent runs with its provider, can call tools
3. **Delegation** — Agent says `NEXT: reviewer` + `TASK: check auth.rs` to hand off
4. **Completion** — Agent says `NEXT: DONE` → response goes to user

## #agent Direct Routing

Start message with `#reviewer` to skip classification and route directly.

## Fellowship YAML

```yaml
name: "my-team"
controller:
  provider: local       # or provider: jev
  model: jevos-v4       # Dedicated zero-shot decision router (~620MB)
agents:
  - name: coder
    provider: gemini
    when: "coding tasks"
    tools: ["*"]
```

## Files

- `agent_loop.rs` — Shared agentic loop — used by both interactive chat and headless exec.
- `config.rs` — Flow configuration — parsed from `.mithril-flow.yaml`.
- `fellowship.rs` — Fellowship — multi-agent orchestration config with GGUF entry classifier.
- `mod.rs` — Multi-agent flow system.
- `orchestrator.rs` — Orchestrator — GGUF entry classifier + agent free-flow + GGUF worker.
- `runner.rs` — FlowRunner — executes the Planner→Tools loop.
- `tokens.rs` — Token tracking — count tokens per provider call, accumulate per-agent.
