# ═══════════════════════════════════════════════════════════════
# Mithril — Multi-Model Orchestration Engine
# Docker deployment for use as backend by Junie, OpenCode, etc.
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
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y \
    ca-certificates libssl3 \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/mithril /usr/local/bin/mithril

# Create config directory
RUN mkdir -p /root/.mithril

# Fellowship config is provided via volume mount (-v .mithril:/root/.mithril)
# No default config copied into the image

# API credentials via environment variables:
#   MITHRIL_KEY_GEMINI=AIza...
#   MITHRIL_KEY_OPENAI=sk-...
#   MITHRIL_KEY_ANTHROPIC=sk-ant-...
#   MITHRIL_KEY_GROQ=gsk_...
#   MITHRIL_KEY_TELEGRAM=123456:ABC...

# Expose the API port
EXPOSE 16180

# Health check
HEALTHCHECK --interval=30s --timeout=5s \
    CMD curl -f http://localhost:16180/health || exit 1

# Default: run as API server
ENTRYPOINT ["mithril"]
CMD ["serve", "--port", "16180"]
