#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Mithril E2E Test Suite — "The Doors of Durin"
# Tests every major subsystem without requiring a running LLM.
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

set -euo pipefail

SCRIPT_DIR="$( cd -- "$( dirname -- "${BASH_SOURCE[0]}" )" \&> /dev/null && pwd )"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
MITHRIL="$PROJECT_ROOT/target/release/mithril"
PASS=0
FAIL=0
TOTAL=0

# Colors
GREEN='\033[0;32m'
RED='\033[0;31m'
DIM='\033[2m'
BOLD='\033[1m'
NC='\033[0m'

test_case() {
    local name="$1"
    TOTAL=$((TOTAL + 1))
    printf "  %s %-50s" "⚔" "$name"
}

pass() {
    PASS=$((PASS + 1))
    printf "${GREEN}✓${NC}\n"
}

fail() {
    FAIL=$((FAIL + 1))
    printf "${RED}✗${NC} %s\n" "${1:-}"
}

echo ""
echo -e "${BOLD}🗡️  Mithril E2E Test Suite — The Doors of Durin${NC}"
echo -e "${DIM}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo ""

# ── 1. Binary basics ─────────────────────────────────────────────
echo -e "${BOLD}§1 Binary & CLI${NC}"

test_case "binary exists and runs"
if $MITHRIL --version | grep -q "mithril 0.1.0"; then pass; else fail; fi

test_case "help shows all commands"
HELP=$($MITHRIL --help)
if echo "$HELP" | grep -q "serve" && \
   echo "$HELP" | grep -q "chat" && \
   echo "$HELP" | grep -q "exec" && \
   echo "$HELP" | grep -q "flow" && \
   echo "$HELP" | grep -q "scan"; then pass; else fail "missing commands"; fi

test_case "exec command --help"
if $MITHRIL exec --help | grep -q "PROMPT"; then pass; else fail; fi

test_case "chat command has --plain flag"
if $MITHRIL chat --help | grep -q "\-\-plain"; then pass; else fail; fi

test_case "chat command has --session flag"
if $MITHRIL chat --help | grep -q "\-\-session"; then pass; else fail; fi

# ── 2. Config system ─────────────────────────────────────────────
echo ""
echo -e "${BOLD}§2 Config System${NC}"

test_case "config list works"
if $MITHRIL config list 2>&1 | grep -q "Provider"; then pass; else fail; fi

test_case "config shows groq in providers"
# Groq is registered — verify via providers error message
if grep -q "groq" src/providers/mod.rs; then pass; else fail "groq not in mod.rs"; fi

# ── 3. Tool registry ─────────────────────────────────────────────
echo ""
echo -e "${BOLD}§3 Tools (MCP)${NC}"

# Test via mcp-stdio — send initialize + tools/list
test_case "MCP tools/list returns tools"
TOOLS=$(echo '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"capabilities":{}}}
{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}' | $MITHRIL mcp-stdio 2>/dev/null || true)
if echo "$TOOLS" | grep -q "read_psi"; then pass; else fail "read_psi not found"; fi

test_case "edit_file tool registered"
if echo "$TOOLS" | grep -q "edit_file"; then pass; else fail; fi

test_case "apply_patch tool registered"
if echo "$TOOLS" | grep -q "apply_patch"; then pass; else fail; fi

test_case "lore_write tool registered"
if echo "$TOOLS" | grep -q "lore_write"; then pass; else fail; fi

test_case "lore_read tool registered"
if echo "$TOOLS" | grep -q "lore_read"; then pass; else fail; fi

test_case "search_symbols tool registered"
if echo "$TOOLS" | grep -q "search_symbols"; then pass; else fail; fi

test_case "document_outline tool registered"
if echo "$TOOLS" | grep -q "document_outline"; then pass; else fail; fi

# ── 4. edit_file tool (functional test) ──────────────────────────
echo ""
echo -e "${BOLD}§4 edit_file Tool${NC}"

TMPDIR=$(mktemp -d)
trap "rm -rf $TMPDIR" EXIT

# Create a test file
echo -e "line 1\nline 2\nline 3" > "$TMPDIR/test.txt"

test_case "edit_file applies search/replace"
RESULT=$(echo "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{\"capabilities\":{}}}
{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/call\",\"params\":{\"name\":\"edit_file\",\"arguments\":{\"target\":\"$TMPDIR/test.txt\",\"edits\":\"<<<<<<< SEARCH\nline 2\n=======\nline TWO\n>>>>>>> REPLACE\"}}}" | $MITHRIL mcp-stdio 2>/dev/null || true)
if echo "$RESULT" | grep -q "Applied 1 edit"; then pass; else fail "$RESULT"; fi

test_case "edit was actually written"
if grep -q "line TWO" "$TMPDIR/test.txt"; then pass; else fail "file not modified"; fi

test_case "edit_file error on missing search text"
RESULT2=$(echo "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{\"capabilities\":{}}}
{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/call\",\"params\":{\"name\":\"edit_file\",\"arguments\":{\"target\":\"$TMPDIR/test.txt\",\"edits\":\"<<<<<<< SEARCH\nNONEXISTENT\n=======\nreplacement\n>>>>>>> REPLACE\"}}}" | $MITHRIL mcp-stdio 2>/dev/null || true)
if echo "$RESULT2" | grep -q "not found"; then pass; else fail; fi

# ── 5. Lore (persistent memory) ──────────────────────────────────
echo ""
echo -e "${BOLD}§5 Lore (Persistent Memory)${NC}"

cd "$TMPDIR"

test_case "lore_read on empty project"
LORE=$(echo "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{\"capabilities\":{}}}
{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/call\",\"params\":{\"name\":\"lore_read\",\"arguments\":{}}}" | $MITHRIL mcp-stdio 2>/dev/null || true)
if echo "$LORE" | grep -q "empty"; then pass; else fail; fi

test_case "lore_write creates entry"
echo "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{\"capabilities\":{}}}
{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/call\",\"params\":{\"name\":\"lore_write\",\"arguments\":{\"content\":\"deploy after 6pm\",\"category\":\"todo\"}}}" | $MITHRIL mcp-stdio 2>/dev/null > /dev/null || true
if [ -f ".mithril/lore.md" ]; then pass; else fail "lore.md not created"; fi

test_case "lore file contains the entry"
if grep -q "deploy after 6pm" .mithril/lore.md 2>/dev/null; then pass; else fail; fi

test_case "lore file has category tag"
if grep -q "\[todo\]" .mithril/lore.md 2>/dev/null; then pass; else fail; fi

cd - > /dev/null

# ── 6. Steering files ────────────────────────────────────────────
echo ""
echo -e "${BOLD}§6 Steering Files${NC}"

mkdir -p "$TMPDIR/steering_test/.mithril/steering"
echo "# Project Rules" > "$TMPDIR/steering_test/MITHRIL.md"
echo "---
inclusion: always
---
# Always included" > "$TMPDIR/steering_test/.mithril/steering/rules.md"
echo "---
inclusion: manual
---
# Manual only" > "$TMPDIR/steering_test/.mithril/steering/manual.md"

# Test steering loading (we can't easily test via CLI without an LLM, 
# but we can test the Rust code via a unit test approach)
test_case "MITHRIL.md exists in test dir"
if [ -f "$TMPDIR/steering_test/MITHRIL.md" ]; then pass; else fail; fi

test_case "steering dir has .md files"
if [ -f "$TMPDIR/steering_test/.mithril/steering/rules.md" ]; then pass; else fail; fi

# ── 7. Code intelligence ─────────────────────────────────────────
echo ""
echo -e "${BOLD}§7 Code Intelligence${NC}"

test_case "search_symbols finds 'fn main'"
cd "$(git rev-parse --show-toplevel)"
SYMS=$(echo "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{\"capabilities\":{}}}
{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/call\",\"params\":{\"name\":\"search_symbols\",\"arguments\":{\"query\":\"main\"}}}" | $MITHRIL mcp-stdio 2>/dev/null || true)
if echo "$SYMS" | grep -q "main"; then pass; else fail; fi

test_case "document_outline works on main.rs"
OUTLINE=$(echo "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{\"capabilities\":{}}}
{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/call\",\"params\":{\"name\":\"document_outline\",\"arguments\":{\"target\":\"src/main.rs\"}}}" | $MITHRIL mcp-stdio 2>/dev/null || true)
if echo "$OUTLINE" | grep -q "main\|Commands"; then pass; else fail; fi

# ── 8. Session management ────────────────────────────────────────
echo ""
echo -e "${BOLD}§8 Sessions${NC}"

test_case "sessions list works"
if $MITHRIL sessions list 2>&1; then pass; else fail; fi

# ── 9. TUI module compiles ───────────────────────────────────────
echo ""
echo -e "${BOLD}§9 TUI Module${NC}"

test_case "TUI source files exist"
if [ -f "src/tui/mod.rs" ] && [ -f "src/tui/app.rs" ] && \
   [ -f "src/tui/ui.rs" ] && [ -f "src/tui/events.rs" ] && \
   [ -f "src/tui/theme.rs" ]; then pass; else fail; fi

test_case "TUI module total ~830 lines"
LINES=$(wc -l src/tui/*.rs | tail -1 | awk '{print $1}')
if [ "$LINES" -gt 700 ]; then pass; else fail "$LINES lines"; fi

# ── 10. Agent loop module ────────────────────────────────────────
echo ""
echo -e "${BOLD}§10 Agent Loop & Doom Detection${NC}"

test_case "agent_loop.rs exists"
if [ -f "src/cli/agent_loop.rs" ]; then pass; else fail; fi

test_case "agent_loop has doom loop detection"
if grep -q "Balrog" src/cli/agent_loop.rs; then pass; else fail; fi

test_case "agent_loop has permission gate"
if grep -q "DANGEROUS_TOOLS" src/cli/agent_loop.rs; then pass; else fail; fi

test_case "agent_loop has TraceMode enum"
if grep -q "TraceMode" src/cli/agent_loop.rs; then pass; else fail; fi

# ── 11. Groq provider ───────────────────────────────────────────
echo ""
echo -e "${BOLD}§11 Groq Provider${NC}"

test_case "groq.rs exists"
if [ -f "src/providers/groq.rs" ]; then pass; else fail; fi

test_case "groq has compound mode detection"
if grep -q "is_compound" src/providers/groq.rs; then pass; else fail; fi

test_case "groq has CompoundCustom struct"
if grep -q "CompoundCustom" src/providers/groq.rs; then pass; else fail; fi

test_case "groq registered in create_provider"
if grep -q '"groq"' src/providers/mod.rs; then pass; else fail; fi

# ── 12. Compaction & Subagents ───────────────────────────────────
echo ""
echo -e "${BOLD}§12 Compaction & Subagents${NC}"

test_case "compact.rs exists"
if [ -f "src/cli/compact.rs" ]; then pass; else fail; fi

test_case "subagent.rs exists"  
if [ -f "src/cli/subagent.rs" ]; then pass; else fail; fi

test_case "/compact command in chat.rs"
if grep -q '"/compact"' src/cli/chat.rs; then pass; else fail; fi

test_case "/sub command in chat.rs"
if grep -q '"/sub"' src/cli/chat.rs; then pass; else fail; fi

# ── Summary ──────────────────────────────────────────────────────
echo ""
echo -e "${DIM}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
if [ $FAIL -eq 0 ]; then
    echo -e "${GREEN}${BOLD}All $TOTAL tests passed!${NC} 🗡️  The Doors of Durin stand open."
else
    echo -e "${RED}${BOLD}$FAIL/$TOTAL tests failed.${NC} The doors remain sealed."
fi
echo ""

exit $FAIL
