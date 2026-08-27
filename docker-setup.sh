#!/bin/bash
# ═══════════════════════════════════════════════════════════════
# Mithril Docker Setup — with Kiro CLI + Gemini multi-agent
# ═══════════════════════════════════════════════════════════════

set -e

echo ""
echo "  ⚔ Mithril Docker Setup"
echo "  ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""

# 1. Create fellowship config
mkdir -p .mithril
cat > .mithril/fellowship.yaml << 'EOF'
name: "kiro-gemini"
description: "Kiro worker + Gemini planner/reviewer"

controller:
  provider: gemini
  model: gemini-2.5-flash
  context_window: 2

agents:
  - name: planner
    provider: gemini
    model: gemini-2.5-flash
    role: "Plans tasks and breaks them into steps"
    when: "planning, task breakdown, architecture decisions"
    can_call: [worker]
    tools: []

  - name: worker
    provider: kiro
    model: claude-sonnet-4.6
    role: "Implements code changes and features"
    when: "coding tasks, implementations, bug fixes"
    can_call: [reviewer]
    tools: ["*"]

  - name: reviewer
    provider: gemini
    model: gemini-2.5-flash
    role: "Reviews code for bugs and quality"
    when: "code review, quality checks"
    can_call: []
    tools: ["read_psi", "grep_files", "git_diff"]
EOF

echo "  ✅ Created .mithril/fellowship.yaml"

# 2. Create .env if not exists
if [ ! -f .env ]; then
    echo "MITHRIL_KEY_GEMINI=" > .env
    echo ""
    echo "  ⚠ Set your Gemini key in .env:"
    echo "    echo \"MITHRIL_KEY_GEMINI=AIzaSy...\" > .env"
    echo ""
fi

# 3. Create kiro data dir
mkdir -p .kiro-data

echo "  ✅ Created .kiro-data/ (persists Kiro login)"
echo ""
echo "  ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""
echo "  Next steps:"
echo ""
echo "  1. Set your Gemini key:"
echo "     echo \"MITHRIL_KEY_GEMINI=AIzaSy...\" > .env"
echo ""
echo "  2. Start the container:"
echo "     docker compose up -d"
echo ""
echo "  3. Login to Kiro CLI (one time only):"
echo "     docker exec -it mithril bash"
echo "     kiro-cli auth login"
echo "     exit"
echo ""
echo "  4. Connect Junie to http://localhost:16180"
echo "     Select model: kiro-gemini:latest"
echo ""
echo "  ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""
