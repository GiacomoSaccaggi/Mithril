# Contributing to Mithril

> *"I will take the Ring, though I do not know the way."* — Frodo

Welcome, traveler. You have found the forges beneath the Misty Mountains where Mithril is wrought. Whether you are a Dwarf of great skill, an Elf of ancient wisdom, or a Hobbit of unexpected courage — all are welcome to contribute.

---

## The Fellowship's Code of Conduct

> *"Even the smallest person can change the course of the future."* — Galadriel

- Be excellent to each other. We are a Fellowship, not a band of Orcs.
- Review code as Gandalf would: firmly but kindly. "You shall not pass" is reserved for actual bugs.
- Credit others. Stolen code is the way of Sauron.

---

## Forging New Mithril (How to Contribute)

### 1. The Quest Begins (Setup)

```bash
# Clone the mines of Moria
git clone https://github.com/GiacomoSaccaggi/mithril.git
cd mithril

# Install the tools of the Dwarves
brew install cmake  # macOS
# or: sudo apt install build-essential cmake  # Linux

# Forge the artifact
cargo build --release

# Light your torch (run tests)
cargo test
```

### 2. Choose Your Quest (Issues)

Before you forge, find or create an issue:

- 🗡️ **Bug** — "A Balrog of Morgoth! What did you say?" (something crashed)
- 🛡️ **Enhancement** — "Speak, friend, and enter" (new feature proposal)
- 📜 **Documentation** — "The tale grew in the telling" (docs improvement)

### 3. Create a Branch (Leave the Shire)

```bash
# Branch naming convention — name it after your quest
git checkout -b feat/palantir-improvements    # new feature
git checkout -b fix/shadow-log-corruption     # bug fix
git checkout -b docs/fellowship-guide         # documentation
```

> *"The road goes ever on and on, down from the door where it began."*

Use prefixes: `feat/`, `fix/`, `docs/`, `refactor/`, `test/`

### 4. Write Your Code (Forge the Ring)

**Style of the Dwarven Smiths:**

- Write the minimal code needed. Dwarves waste no metal.
- Name things clearly. `lazy_model_manager` not `lmm`. We are not speaking Black Speech.
- Comments are for *why*, not *what*. The code speaks for itself, like Treebeard — slowly but clearly.
- Match existing patterns. Consistency is the Mithril-coat of maintainability.

**The Three Laws of Mithril Code:**

1. **It shall compile.** `cargo check` must pass. No exceptions. Not even for Wizards.
2. **It shall not leak.** No hardcoded paths, no secrets, no personal data in commits.
3. **It shall be tested.** If you add a function, add a test. Untested code is like going to Mordor without Sam.

### 5. Commit Messages (Write in the Runes)

Format: `type: short description`

```
feat: add Rohan provider for horse-speed inference
fix: shadow log no longer corrupts on concurrent writes
docs: update fellowship configuration guide
refactor: simplify orchestrator directive parsing
test: add unit tests for BM25 scoring
```

Keep it under 70 characters. Gandalf is concise when it matters.

### 6. Open a Pull Request (Council of Elrond)

```bash
git push -u origin feat/your-quest-name
gh pr create
```

Your PR description should answer:
- **What** does this change?
- **Why** is it needed?
- **How** can it be tested?

> *"Nine companions. So be it. You shall be the Fellowship of the Ring."*

---

## The Crafts (What You Can Add)

### Adding a New Provider (Summoning a New Wizard)

Each provider lives in `src/providers/` and implements `ChatProvider`:

```rust
// src/providers/gandalf.rs
pub struct GandalfProvider { /* ... */ }

#[async_trait]
impl ChatProvider for GandalfProvider {
    fn name(&self) -> &str { "gandalf" }
    fn model(&self) -> &str { &self.model }
    async fn chat(&self, messages: &[ChatMessage]) -> Result<String> { /* ... */ }
    async fn chat_stream(&self, messages: &[ChatMessage], on_chunk: Box<dyn Fn(StreamChunk) + Send>) -> Result<String> { /* ... */ }
    async fn is_available(&self) -> bool { /* ... */ }
}
```

Then register it in `create_provider_with_model()` in `src/providers/mod.rs`.

### Adding a New Tool (Forging a New Weapon)

Tools live in `src/tools/implementations.rs`:

```rust
pub struct PalantirVisionTool { /* ... */ }

impl Tool for PalantirVisionTool {
    fn name(&self) -> &'static str { "palantir_vision" }
    fn description(&self) -> &'static str { "See far-off things, as Denethor once did" }
    fn parameters(&self) -> Vec<ToolParam> { /* ... */ }
    fn execute(&self, args: &HashMap<String, String>) -> ToolResult { /* ... */ }
}
```

Register it in `create_default_registry()` in `src/tools/mod.rs`.

### Adding a New Model (Mining Deeper)

Add an entry to `MODELS` in `src/engine/model_catalog.rs`:

```rust
ModelInfo {
    id: "mithrandir-7b",
    display_name: "Mithrandir 7B",
    file_name: "mithrandir-7b-Q4_K_M.gguf",
    url: "https://huggingface.co/...",
    size_mb: 4500,
    template: "chatml",
}
```

---

## The Rules of the Road

### Branch Protection (The Gates of Gondor)

`main` is protected. You cannot push directly — not even if you are the King returned.

- All changes go through Pull Requests
- Force push is forbidden (like using the One Ring)
- Linear history enforced (no merge commits)

### What NOT to Commit (The Forbidden Pool)

- API keys or secrets of any kind
- Personal file paths (`/Users/your-name/...`)
- IDE-specific files (`.idea/`, `.vscode/`)
- Build artifacts (`target/`, `*.gguf`)
- Temporary files (`tmp/`)

These are in `.gitignore`. Respect it as you would the borders of Lothlórien.

---

## Architecture Map (The Map of Middle-Earth)

```
src/
├── main.rs          # The Shire (where the journey starts)
├── engine/          # Khazad-dûm (deep inference mines)
├── providers/       # The Five Istari (LLM backends)
├── flow/            # Rivendell (orchestration council)
├── tools/           # The Armory (24 weapons)
├── operators/       # The Rangers (execute in the wild)
├── api/             # The Beacons (HTTP signals)
├── tui/             # Minas Tirith (the visible city)
├── session/         # Session persistence and handoff
├── index/           # The Palantír (BM25 search)
└── config/          # The Vaults (encrypted treasures)
```

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for the full map of the realms.

---

## Testing (Proving Your Worth)

```bash
# Unit tests (the training grounds)
cargo test

# Build check (the gates must hold)
cargo check

# Format check (the customs of the Elves)
cargo fmt --check

# Lint check (the wisdom of the Wise)
cargo clippy

# Documentation (the libraries of Minas Tirith)
cargo doc --open
```

---

## Documentation (The Great Libraries)

If you change functionality, update the relevant documentation:

| Change | Update |
|--------|--------|
| New tool | [docs/TOOLS.md](docs/TOOLS.md) |
| New command | [docs/CLI.md](docs/CLI.md) |
| Security change | [docs/SECURITY.md](docs/SECURITY.md) |
| API change | [docs/API.md](docs/API.md) |
| New provider | [docs/PROVIDERS.md](docs/PROVIDERS.md) |

---

## Recognition (The Hall of Fame)

Contributors are honored in the tradition of the Dwarves — by the quality of their craft, not the quantity of their commits.

> *"All we have to decide is what to do with the time that is given us."* — Gandalf

---

## Questions?

Open an issue. We don't have eagles to carry you to the answer, but we'll do our best.

> *"May it be a light to you in dark places, when all other lights go out."* — Galadriel
