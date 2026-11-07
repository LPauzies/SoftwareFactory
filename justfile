# WAGNER // Autonomous Software Factory — Justfile
# Run `just` (or `just up`) to start the full stack (Rust backend + Vue 3 frontend) on http://localhost:8888 with one command.

set shell := ["bash", "-euo", "pipefail", "-c"]

# Default recipe: start the full stack locally with one command (`just`)
default: up

# Start both Rust backend (port 3001) and Vue 3 dev server (http://localhost:8888)
up:
    ./start.sh

# Alias for `just up`
dev: up

# Build the Vue 3 production bundle and serve the entire stack (SPA + API) from the Rust binary on http://localhost:8888
prod: build
    cd backend && PORT=8888 STATIC_DIR=../frontend/dist cargo run --release

# Build and start the unified Docker stack in the background on http://localhost:8888
docker-up:
    docker compose up --build -d
    @echo "WAGNER running at http://localhost:8888"

# Stop the Docker stack
docker-down:
    docker compose down

# Build both the Vue 3 frontend and Rust backend in release mode
build:
    cd frontend && npm install && npm run build
    cd backend && cargo build --release

# Run the Rust backend unit & integration test suite
test:
    cd backend && cargo test --all-targets -- --nocapture

# Run fast compile & type checks across backend and frontend
check:
    cd backend && cargo check
    cd frontend && npm run build

# Clean build artifacts
clean:
    cd backend && cargo clean
    rm -rf frontend/dist
