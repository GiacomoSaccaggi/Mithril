# ═══════════════════════════════════════════════════════════════
# Mithril — Multi-Model Orchestration Engine
# Docker deployment for use as backend by Junie, OpenCode, etc.
# Includes Kiro CLI for CLI provider support.
# ═══════════════════════════════════════════════════════════════

# Stage 1: Build
FROM rust:latest AS builder

RUN apt-get update && apt-get install -y \
    cmake g++ pkg-config libssl-dev libclang-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY . .
RUN cargo build --release

# Stage 2: Runtime
FROM debian:sid-slim

RUN apt-get update && apt-get install -y \
    ca-certificates libssl3 curl git libgomp1 \
    && rm -rf /var/lib/apt/lists/*

# Install Mithril
COPY --from=builder /app/target/release/mithril /usr/local/bin/mithril

# Install Kiro CLI
RUN curl -fsSL https://kiro.dev/install.sh | bash 2>/dev/null || true
ENV PATH="/root/.local/bin:${PATH}"

# Create directories
RUN mkdir -p /root/.mithril /root/.kiro

# API credentials via environment variables:
#   MITHRIL_KEY_GEMINI=AIza...
#   MITHRIL_KEY_OPENAI=sk-...
#   MITHRIL_KEY_ANTHROPIC=sk-ant-...
#   MITHRIL_KEY_GROQ=gsk_...

# Expose the API port
EXPOSE 16180

# Health check
HEALTHCHECK --interval=30s --timeout=5s \
    CMD curl -f http://localhost:16180/health || exit 1

# Default: run as API server
ENTRYPOINT ["mithril"]
CMD ["serve", "--port", "16180"]
