# PhotoMind

The most versatile AI-powered photo assistant. Designed for NAS/Docker deployment.

- **AI Search** — Find photos by natural language or image similarity, powered by embedding vectors
- **AI Management** — Chat with an AI agent to organize, move, and manage your photos
- **AI Chat** — Conversational interface for photo operations with tool confirmation workflow
- **Universal Compatibility** — Integrate with any photo album app via CLI or HTTP API tools

Rust backend + React frontend. Supports Anthropic, Google, OpenAI, and any OpenAI-compatible provider.

## Prerequisites

- [Rust](https://rustup.rs/) (1.94+)
- [Node.js](https://nodejs.org/) (22+)
- Build tools: `sudo apt install build-essential pkg-config libssl-dev`

## Quick Start

```bash
cd ~/ETProject/PhotoMind

# Install frontend dependencies (first time only)
cd web && npm install && cd ..

# Build and run
make run
```

Open **http://localhost:8080**

## Run Methods

### 1. Make (Recommended)

```bash
# Development mode — backend debug + frontend HMR hot reload
make dev        # then open http://localhost:5173

# Production mode — build everything and start
make run        # then open http://localhost:8080

# Build only (no start)
make build

# Clean all build artifacts
make clean
```

### 2. Manual — Backend Only

```bash
# Debug build + run
cargo run

# Or release build + run
cargo build --release
./target/release/photomind
```

Backend listens on **http://localhost:8080**. Serves the frontend from `web/dist/` (must be built first).

### 3. Manual — Frontend Only

```bash
cd web

# Development server with hot reload (proxies API to :8080)
npm run dev     # http://localhost:5173

# Production build (outputs to web/dist/)
npm run build
```

### 4. Docker

```bash
# Edit docker-compose.yml to set your photos directory, then:
docker compose up --build
```

## Environment Variables

| Variable | Default | Description |
|---|---|---|
| `PHOTOMIND_DATA_DIR` | `data` | SQLite database + thumbnail cache |
| `PHOTOMIND_ADDR` | `0.0.0.0:8080` | Server listen address |
| `RUST_LOG` | `info` | Log level (`debug`, `info`, `warn`, `error`) |

## Setup After First Launch

1. Open **Settings** page
2. Add **Scan Directories** — paths to your photo folders
3. Configure **Embedding Model** — Google API URL + Key, select `gemini-embedding`
4. Configure **Agent Model** — choose provider (Anthropic/Google/OpenAI), enter URL + Key, select model
5. Click **Save Settings** → **Scan & Embed**
6. Go to **Search** to find photos, or **Chat** to talk with the agent

## Project Structure

```
PhotoMind/
├── crates/
│   ├── server/        Axum HTTP server + API routes
│   ├── core/          Scanner, embedding, search, agent, file watcher
│   ├── storage/       SQLite database layer (photos, embeddings, configs, tools, chat)
│   └── tools/         Tool error types
├── web/               React frontend (Vite + TypeScript + Tailwind)
├── Dockerfile         Multi-stage build
├── docker-compose.yml
└── Makefile
```
