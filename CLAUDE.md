# PhotoMind — CLAUDE.md

## What is this project

PhotoMind is an AI-powered photo search agent. Users describe a photo in natural language (or upload an image), the system finds matching photos via embedding similarity. An agent (LLM) can also call tools to manage photos (move files, create folders, integrate with external album apps).

Designed for NAS/Docker deployment — web UI for management, no native client needed.

## Tech Stack

- **Backend**: Rust (edition 2021), Cargo workspace
- **HTTP Framework**: axum 0.8
- **Database**: SQLite via sqlx 0.8 (WAL mode)
- **Frontend**: React 19 + TypeScript + Vite + Tailwind CSS v4
- **Routing**: react-router-dom
- **Icons**: lucide-react
- **Image processing**: `image` crate + `kamadak-exif`
- **File watching**: `notify` crate
- **HTTP client**: `reqwest`
- **Deployment**: Docker multi-stage build

## Project Root

`~/ETProject/PhotoMind` (Linux FS for cargo performance, NOT on /mnt/f)

## Directory Structure

```
PhotoMind/
├── Cargo.toml                    # Workspace root — members: server, core, storage, tools
├── Makefile                      # make build / make run / make dev / make clean
├── Dockerfile                    # Multi-stage: rust → node → debian-slim
├── docker-compose.yml
│
├── crates/
│   ├── server/                   # Binary crate — "photomind"
│   │   └── src/
│   │       ├── main.rs           # Entry: DB init, register tools, load index, start axum, spawn scan+embed, file watcher
│   │       └── api/
│   │           ├── mod.rs
│   │           ├── settings.rs   # GET/PUT /api/settings, POST fetch embedding/agent models
│   │           ├── tools.rs      # CRUD /api/tools, toggle enable/disable
│   │           ├── status.rs     # GET /api/status, POST /api/scan (trigger scan+embed)
│   │           ├── search.rs     # POST /api/search (text), POST /api/search/image, GET /api/photos/{id}/thumbnail
│   │           └── chat.rs       # POST /api/chat, POST /api/chat/confirm-tool, builtin+external tool execution
│   │
│   ├── core/                     # Library crate — business logic
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── error.rs          # CoreError
│   │       ├── scanner/
│   │       │   ├── mod.rs
│   │       │   └── mod_scanner.rs # PhotoScanner: recursive dir walk, EXIF extraction, SHA256 hash, insert to DB
│   │       ├── thumbnail.rs      # ThumbnailGenerator: resize to 400px JPEG, cache in data/thumbnails/
│   │       ├── embedding.rs      # EmbeddingClient (Google embedContent API), EmbeddingPipeline (batch process)
│   │       ├── search.rs         # VectorIndex: in-memory brute-force cosine similarity, normalized vectors
│   │       ├── watcher.rs        # FileWatcher: notify crate, auto-scan on file create/modify
│   │       └── agent/
│   │           ├── mod.rs
│   │           ├── types.rs      # AgentMessage, AgentResponse, AgentToolCall, ToolDefinition, Role
│   │           ├── provider.rs   # AgentProvider: 4 backends (Anthropic, Google, OpenAI, OpenAI-compat)
│   │           └── engine.rs     # AgentEngine: system prompt, tool chain, delete intent filtering
│   │
│   ├── storage/                  # Library crate — database layer
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── db.rs             # Database: SQLite init, WAL mode, all CREATE TABLE migrations
│   │       ├── error.rs          # StorageError
│   │       ├── models.rs         # Photo, Embedding, Config, ToolDef, ToolExecution, ChatMessage (+ New* variants)
│   │       └── repo/
│   │           ├── mod.rs
│   │           ├── photos.rs     # PhotoRepo: insert, get_by_id/path/hash, list_unembedded, mark_embedded, update_path, count
│   │           ├── embeddings.rs # EmbeddingRepo: insert, load_all (for index), f32 <-> BLOB conversion
│   │           ├── configs.rs    # ConfigRepo: get/set/get_all/delete key-value store
│   │           ├── tools.rs      # ToolRepo: upsert, list, list_enabled, set_enabled, delete
│   │           └── chat.rs       # ChatRepo: insert, get_session_messages
│   │
│   └── tools/                    # Library crate — tool error types (lightweight)
│       └── src/
│           ├── lib.rs
│           └── error.rs          # ToolError: NotFound, Disabled, DeleteNotAllowed, etc.
│
└── web/                          # React frontend
    ├── vite.config.ts            # Tailwind plugin, @ alias, proxy /api → :8080
    ├── src/
    │   ├── main.tsx
    │   ├── index.css             # Tailwind v4 @theme, light/dark mode, purple accent
    │   ├── App.tsx               # BrowserRouter, NavBar (Search/Chat/Settings), Routes
    │   ├── lib/api.ts            # apiFetch helper, apiUrl
    │   ├── pages/
    │   │   ├── SearchPage.tsx    # Google-style search box, image upload, result grid with hover overlay
    │   │   ├── ChatPage.tsx      # Chat UI, tool confirmation cards (Confirm/Cancel buttons)
    │   │   └── SettingsPage.tsx  # Scan dirs, embedding model, agent model (4 providers), tools list with search
    │   └── components/
    │       └── AddToolDialog.tsx  # Modal form: HTTP API or CLI tool, URL/headers/body templates, JSON Schema
    └── dist/                     # Built output (served by axum in production)
```

## Database Schema (SQLite)

6 tables created in `storage/src/db.rs`:
- **photos** — file_path (unique), file_name, file_size, width, height, format, taken_at, file_hash, embedded (bool)
- **embeddings** — photo_id (FK), vector (BLOB, f32 array), model_name
- **configs** — key-value store (JSON values), used for all settings
- **tools** — id, name, description, category (builtin/external), enabled, config (JSON), schema (JSON)
- **tool_executions** — tool_id, params, result, status (pending_confirm/confirmed/executed/failed/cancelled)
- **chat_messages** — session_id, role, content, metadata (JSON)

## API Endpoints

| Method | Path | Description |
|--------|------|-------------|
| GET | /api/settings | Get all config key-values |
| PUT | /api/settings | Update config key-values |
| POST | /api/settings/embedding-models | Fetch available embedding models from provider |
| POST | /api/settings/agent-models | Fetch available agent models from provider |
| GET | /api/tools | List all tools |
| POST | /api/tools | Create/upsert a tool |
| PATCH | /api/tools/{id} | Toggle tool enabled/disabled |
| DELETE | /api/tools/{id} | Delete a tool |
| GET | /api/status | System status (photo count, embedding count, index size) |
| POST | /api/scan | Trigger background scan + embed |
| POST | /api/search | Text search (embedding similarity) |
| POST | /api/search/image | Image search (multipart upload) |
| GET | /api/photos/{id} | Get photo metadata |
| GET | /api/photos/{id}/thumbnail | Get photo thumbnail (JPEG) |
| POST | /api/chat | Send message to agent |
| POST | /api/chat/confirm-tool | Confirm or cancel a pending tool execution |

## Architecture Decisions

1. **Vector index**: Brute-force cosine similarity in memory. All embeddings loaded on startup. Sufficient for 10k-100k photos. Can upgrade to HNSW later.
2. **Embedding storage**: f32 arrays serialized as BLOB in SQLite, converted on load.
3. **Agent providers**: Unified trait with 4 implementations. Response parsed into common AgentResponse (content + tool_calls).
4. **Tool confirmation**: Agent returns tool calls → saved as pending_confirm in tool_executions → frontend shows confirm/cancel → user confirms → backend executes.
5. **No delete tools**: System prompt forbids deletion. `filter_delete_intent()` in engine.rs strips any tool call with "delete"/"remove"/"trash" in the name. Agent suggests moving to a folder instead.
6. **External tools**: Configured via HTTP API templates (method, url, headers, body with {param} placeholders) or CLI command templates. Stored in tools table.
7. **File watcher**: `notify` crate watches configured scan dirs. On file create/modify, triggers re-scan of parent directory.

## Config Keys (stored in configs table)

- `scan_dirs` — JSON array of directory paths
- `embedding_url` — Base URL for embedding API
- `embedding_key` — API key for embedding
- `embedding_model` — Model name (e.g. "models/gemini-embedding")
- `agent_provider` — "anthropic" | "google" | "openai" | "openai_compat"
- `agent_url` — Base URL for agent API
- `agent_key` — API key for agent
- `agent_model` — Model name

## Built-in Tools (registered on startup in main.rs)

- `builtin:search_photos` — search by natural language query
- `builtin:move_file` — move photo to destination path
- `builtin:create_folder` — create a directory
- `builtin:get_photo_info` — get photo metadata by ID

## Build & Run

```bash
make dev      # Development: cargo run + npm run dev (HMR on :5173)
make run      # Production: build all + start on :8080
make build    # Build only
make clean    # Clean all
```

## Key Dependencies (workspace Cargo.toml)

axum 0.8, sqlx 0.8 (sqlite), tokio, reqwest, serde/serde_json, image 0.25, kamadak-exif 0.5, notify 7, sha2, base64, chrono, uuid, tracing
