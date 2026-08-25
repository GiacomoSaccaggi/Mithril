# Wisdom in Token Usage

> *"All we have to decide is what to do with the time that is given us."* — Gandalf

Tokens are the currency of the realm. This guide covers how Mithril manages token budgets and how to use them wisely.

---

## Understanding Tokens

### What Is a Token?

A token is roughly 4 characters of English text, or 0.75 words. Code tends to be more token-dense than prose.

| Content | ~Tokens |
|---------|---------|
| "hello" | 1 |
| "Hello, world!" | 3 |
| 100 lines of code | 500-1000 |
| 1 page of text | 300-500 |

### Token Costs

Every interaction has two token costs:

| Type | Description |
|------|-------------|
| **Input** | Your message + context + system prompt |
| **Output** | Model's response |

Cloud providers typically charge 3-10x more for output tokens.

---

## Token Budget System

### Per-Agent Budgets

Define limits for each agent in fellowship config:

```yaml
agents:
  - name: worker
    provider: gemini
    model: gemini-2.5-flash
    token_budget: 50000
    
  - name: reviewer
    provider: anthropic
    model: claude-sonnet-4-20250514
    token_budget: 20000
```

### Session Budget

Overall session limit:

```yaml
token_budget: 100000  # Total for all agents
```

### Budget Enforcement

| Threshold | Behavior |
|-----------|----------|
| 80% used | Warning displayed |
| 95% used | New requests require confirmation |
| 100% used | Requests blocked until reset |

### Checking Usage

```
/tokens
```

Output:
```
Token Usage (this session):
  gemini-2.5-flash:    Input: 12,500 / 50,000  Output: 8,200
  local/qwen-7b:       Input: 3,200           Output: 2,100
  
Session Total: 26,000 / 100,000 (26%)

Estimated Cost: $0.42
```

---

## Reducing Token Usage

### 1. Use Local Models for Simple Tasks

```yaml
controller:
  provider: local
  model: qwen-1.5b  # Routing is cheap locally
```

Local models have zero token cost for cloud budgets.

### 2. Be Specific in Requests

❌ Wasteful:
```
Look at all my code and tell me everything about it
```

✅ Efficient:
```
Review src/auth/jwt.rs for security issues
```

### 3. Use Plan Mode for Exploration

Plan mode restricts write operations but also uses simpler prompts, reducing input tokens.

### 4. Leverage the Palantír

The BM25 index finds relevant context efficiently. Instead of including entire files, Mithril injects only the relevant chunks.

### 5. Clear Context When Switching Tasks

```
/clear
```

Long conversation histories inflate input tokens.

---

## Context Window Management

### Window Sizes

| Provider | Typical Window |
|----------|----------------|
| Local (7B) | 4K-8K |
| Gemini | 1M-2M |
| OpenAI | 128K |
| Anthropic | 200K |

### Automatic Truncation

When context exceeds the window:

1. Oldest messages removed first
2. System prompt always preserved
3. Recent tool results preserved
4. User gets notification

### Manual Control

```
/context
```

Shows current context size and breakdown.

---

## Token-Efficient Patterns

### Pattern 1: Progressive Disclosure

Start broad, then narrow:

```
1. "What files handle authentication?"
   → Model returns file list (few tokens)

2. "Explain src/auth/jwt.rs"
   → Focus on specific file
```

### Pattern 2: Batch Operations

❌ Multiple small requests:
```
"Read src/a.rs"
"Read src/b.rs"
"Read src/c.rs"
```

✅ Single batch request:
```
"Read src/a.rs, b.rs, and c.rs"
```

### Pattern 3: Template Reuse

For repetitive tasks, define custom commands:

```yaml
commands:
  /review-file:
    prompt: "Review {arg} for: security issues, error handling, code style"
```

The prompt template is shorter than re-explaining each time.

### Pattern 4: Checkpoint and Continue

For long tasks:
```
"Continue from where you left off"
```

Model doesn't need full re-explanation.

---

## Cost Estimation

### Rough Provider Pricing

| Provider | Input (1M tok) | Output (1M tok) |
|----------|---------------|-----------------|
| Gemini Flash | $0.075 | $0.30 |
| Gemini Pro | $1.25 | $5.00 |
| GPT-4o | $2.50 | $10.00 |
| Claude Sonnet | $3.00 | $15.00 |
| Local | Free | Free |

*Prices approximate, check provider for current rates.*

### Session Cost Display

Enable cost tracking:

```yaml
tokens:
  show_costs: true
  currency: USD
```

Now `/tokens` shows estimated costs.

---

## Token Metrics

### What's Measured

| Metric | Description |
|--------|-------------|
| `input_tokens` | Tokens sent to model |
| `output_tokens` | Tokens received from model |
| `context_tokens` | Tokens from Palantír injection |
| `system_tokens` | System prompt tokens |
| `tool_tokens` | Tool call/result tokens |

### Export Metrics

```bash
mithril sessions --export "session-name" --format json
```

Includes full token breakdown per message.

---

## Optimizing System Prompts

### Default System Prompt

Mithril's default system prompt is optimized for efficiency while maintaining capability.

### Custom System Prompts

In `MITHRIL.md`:

```markdown
# Project: MyApp

## Context
A REST API in Rust using Actix-web.

## Style
- Use async/await
- Error handling with anyhow
- Tests with #[tokio::test]
```

Keep it concise — every word costs tokens.

### System Prompt Caching

Providers like Anthropic support prompt caching:

```yaml
providers:
  anthropic:
    cache_system_prompt: true
```

Cached prompts are heavily discounted on repeat calls.

---

## Multi-Agent Token Strategy

### Classifier Efficiency

The GGUF classifier should be tiny:

```yaml
controller:
  provider: local
  model: qwen-1.5b
  context_window: 2  # Only see last 2 messages
```

It routes requests at minimal token cost.

### Agent Specialization

| Agent | Model | Purpose |
|-------|-------|---------|
| classifier | qwen-1.5b | Routing (1-2K tokens) |
| worker | gemini-flash | Fast tasks (5-20K tokens) |
| reviewer | claude-sonnet | Complex review (10-50K tokens) |

Don't use expensive models for simple tasks.

---

## Monitoring and Alerts

### Usage Alerts

```yaml
tokens:
  alerts:
    - threshold: 50000
      action: warn
    - threshold: 90000
      action: confirm
    - threshold: 100000
      action: block
```

### Session Reports

End of session summary:

```
Session ended.
Duration: 45 minutes
Messages: 23
Tokens: 34,500 (input: 12,000, output: 22,500)
Estimated cost: $0.89
Most used: gemini-2.5-flash (28,000 tokens)
```

---

## Best Practices Summary

| Practice | Token Impact |
|----------|--------------|
| Use local classifier | -90% routing cost |
| Specific questions | -50% input tokens |
| Palantír context | -70% vs full files |
| Clear between tasks | -30% accumulated context |
| Plan mode for exploration | -20% overall |
| Batch similar operations | -40% overhead |
| Template custom commands | -25% repeated prompts |

---

> *"A wizard is never late, nor is he early. He arrives precisely when he means to."*
