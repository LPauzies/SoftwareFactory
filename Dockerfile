# =============================================================================
# Stage 1: Build Vue 3 (Vite + TypeScript + Pinia) Frontend
# =============================================================================
FROM node:20-bookworm-slim AS frontend-builder

WORKDIR /app/frontend

COPY frontend/package.json frontend/package-lock.json* ./
RUN npm install

COPY frontend/ ./
RUN npm run build

# =============================================================================
# Stage 2: Build Rust (Axum + Tokio) Orchestrator Backend
# =============================================================================
FROM rust:1-bookworm AS backend-builder

WORKDIR /app/backend

COPY backend/Cargo.toml backend/Cargo.lock* ./
COPY backend/src ./src

RUN cargo build --release

# =============================================================================
# Stage 3: Unified Production Runtime (Frontend Static SPA + Rust API Server)
# =============================================================================
FROM debian:bookworm-slim AS runtime

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    curl \
    git \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy compiled Rust binary
COPY --from=backend-builder /app/backend/target/release/wagner-backend /app/wagner-backend

# Copy compiled Vue 3 production assets served directly by Axum
COPY --from=frontend-builder /app/frontend/dist /app/static

# Copy Antigravity workspace skills
COPY .agents /app/.agents

# Create persistent directories for state & generated release artifacts
RUN mkdir -p /app/data /app/workspaces

ENV PORT=8888
ENV STATIC_DIR=/app/static
ENV AGY_BIN=/usr/local/bin/agy
ENV RUST_LOG=wagner_backend=info,tower_http=info

EXPOSE 8888

HEALTHCHECK --interval=15s --timeout=5s --start-period=5s --retries=3 \
    CMD curl -fsS http://127.0.0.1:8888/api/health || exit 1

CMD ["/app/wagner-backend"]
