# PhotoMind 实现计划

## 项目总览

PhotoMind：一个 AI 驱动的照片搜索智能体。Rust 后端 + React 前端，面向 NAS/Docker 部署。

核心能力：对照片进行 embedding 建立索引 → 用户通过自然语言或以图搜图快速检索 → Agent 通过 tool 系统执行操作（移动、创建文件夹、集成外部相册 app）。

---

## 阶段 1：项目骨架与基础设施

### 1.1 Rust 后端项目初始化
- 使用 `cargo init` 创建项目，采用 workspace 结构：
  ```
  /
  ├── Cargo.toml          (workspace)
  ├── crates/
  │   ├── server/          (HTTP server, API routes, main entry)
  │   ├── core/            (业务逻辑：embedding, search, agent)
  │   ├── storage/         (数据库操作, 向量索引)
  │   └── tools/           (tool 系统框架 + 内置 tools)
  ├── web/                 (React 前端)
  ├── Dockerfile
  └── docker-compose.yml
  ```
- 核心依赖：
  - `axum` — HTTP 框架
  - `sqlx` + SQLite — 元数据存储（照片信息、配置、tool 定义）
  - `serde` / `serde_json` — 序列化
  - `tokio` — 异步运行时
  - `reqwest` — HTTP 客户端（调用 embedding/agent API）
  - `image` — 图片处理（缩略图、格式检测）
  - `notify` — 文件系统监听（自动发现新照片）

### 1.2 React 前端项目初始化
- `web/` 目录，使用 Vite + React + TypeScript
- UI 库：Tailwind CSS + shadcn/ui（轻量、美观）
- 核心页面规划：
  - **搜索页**（首页）：搜索框 + 结果网格
  - **设置页**：模型配置、Tool 管理、扫描目录配置
  - **聊天页**：与 Agent 交互的对话界面

### 1.3 Docker 配置
- 多阶段构建 Dockerfile：Rust 编译 → 前端构建 → 最终运行镜像
- docker-compose.yml：挂载照片目录、数据卷

---

## 阶段 2：核心数据层

### 2.1 数据库 Schema (SQLite)
```sql
-- 照片元数据
photos (
  id          INTEGER PRIMARY KEY,
  file_path   TEXT UNIQUE NOT NULL,
  file_name   TEXT NOT NULL,
  file_size   INTEGER,
  width       INTEGER,
  height      INTEGER,
  format      TEXT,            -- jpeg, png, webp, etc.
  taken_at    DATETIME,        -- EXIF 拍摄时间
  created_at  DATETIME,
  updated_at  DATETIME,
  file_hash   TEXT,            -- 用于去重和变更检测
  embedded    BOOLEAN DEFAULT FALSE
)

-- 向量存储（embedding）
embeddings (
  id          INTEGER PRIMARY KEY,
  photo_id    INTEGER REFERENCES photos(id),
  vector      BLOB NOT NULL,   -- f32 数组序列化
  model_name  TEXT NOT NULL,    -- 使用的 embedding 模型
  created_at  DATETIME
)

-- 系统配置
configs (
  key         TEXT PRIMARY KEY,
  value       TEXT NOT NULL,    -- JSON
  updated_at  DATETIME
)

-- Tool 定义
tools (
  id          TEXT PRIMARY KEY,  -- 如 "builtin:move_file", "external:immich:move_to_album"
  name        TEXT NOT NULL,
  description TEXT,
  category    TEXT,              -- builtin / external
  enabled     BOOLEAN DEFAULT TRUE,
  config      TEXT,              -- JSON: API endpoint, CLI command 等
  schema      TEXT,              -- JSON: tool 的参数 schema
  created_at  DATETIME,
  updated_at  DATETIME
)

-- Tool 执行日志
tool_executions (
  id          INTEGER PRIMARY KEY,
  tool_id     TEXT REFERENCES tools(id),
  params      TEXT,             -- JSON
  result      TEXT,             -- JSON
  status      TEXT,             -- pending_confirm / confirmed / executed / failed
  created_at  DATETIME,
  confirmed_at DATETIME
)

-- 聊天历史
chat_messages (
  id          INTEGER PRIMARY KEY,
  session_id  TEXT NOT NULL,
  role        TEXT NOT NULL,     -- user / assistant / system / tool
  content     TEXT NOT NULL,
  metadata    TEXT,              -- JSON: 图片引用、tool 调用等
  created_at  DATETIME
)
```

### 2.2 向量索引
- 使用内存中的向量索引实现（基于 brute-force 或 HNSW）
- 启动时从 SQLite 加载所有 embedding 到内存
- 照片数量级预估：万~十万级，内存索引完全可行
- 考虑使用 `usearch` 或 `hnsw_rs` crate，或手写余弦相似度 brute-force（初期简单方案）
- 初期方案：直接 brute-force 余弦相似度（简洁可靠），后续可升级为 HNSW

---

## 阶段 3：照片扫描与 Embedding

### 3.1 照片扫描器
- 用户在设置中配置扫描目录（支持多个）
- 扫描策略：
  - 首次全量扫描
  - 后续增量扫描：通过 file_hash 检测变更
  - `notify` crate 监听文件变更实时更新
- 支持格式：JPEG, PNG, WebP, HEIC, TIFF, BMP, GIF
- 提取 EXIF 信息：拍摄时间、GPS（如有）、相机信息
- 生成缩略图并缓存

### 3.2 Embedding 流水线
- 扫描到新照片 → 加入 embedding 队列
- 队列处理器：
  - 读取图片 → 调用 embedding API（Google gemini-embedding）
  - 存储 embedding 向量到数据库
  - 更新内存索引
- 支持批量处理，带速率限制
- Embedding API 调用格式（Google）：
  ```
  POST {base_url}/v1/models/{model}:embedContent
  Authorization: Bearer {api_key}
  Body: { "content": { "parts": [{ "inline_data": { "mime_type": "image/jpeg", "data": "<base64>" } }] } }
  ```
- 断点续传：记录已 embed 的照片，中断后可续

### 3.3 模型配置（Embedding）
- 设置页：输入 URL base + API Key
- 自动发现模型：`GET {base_url}/v1/models`，筛选 embedding 类型
- 默认推荐 `gemini-embedding`
- 配置存储在 `configs` 表

---

## 阶段 4：搜索系统

### 4.1 文本搜索
- 用户输入文本 → 调用 embedding API 获取文本向量
- 与内存中的图片向量做余弦相似度计算
- 返回 Top-K 结果，包含：
  - 缩略图
  - 文件名、路径
  - 拍摄时间
  - 文件大小、分辨率
  - 相似度分数

### 4.2 以图搜图
- 用户上传图片 → 调用 embedding API 获取向量
- 同文本搜索一样做相似度检索
- 返回相似照片列表

### 4.3 搜索 API
```
POST /api/search
{
  "query": "站在樱花树下的粉发模特",  // 文本搜索
  "image": "<base64>",                // 以图搜图（二选一或组合）
  "limit": 20,
  "offset": 0
}
```

---

## 阶段 5：Agent 系统

### 5.1 Agent 核心
- 与 LLM 对话，支持 4 种 provider：
  - **Anthropic**: `POST {url}/v1/messages`，tool_use 原生支持
  - **Google**: `POST {url}/v1beta/models/{model}:generateContent`，function calling
  - **OpenAI Chat**: `POST {url}/v1/chat/completions`，function calling
  - **OpenAI Responses**: `POST {url}/v1/responses`，tools
- 统一的 provider 抽象层：
  ```rust
  trait AgentProvider {
      async fn chat(&self, messages: Vec<Message>, tools: Vec<ToolDef>) -> Result<AgentResponse>;
      async fn list_models(&self) -> Result<Vec<Model>>;
  }
  ```

### 5.2 Agent 模型配置
- 设置页：选择 provider → 输入 URL + API Key
- 自动获取模型列表（调用各家 list models API）
- 用户选择模型，或手动输入模型名
- 配置存储在 `configs` 表

### 5.3 Tool Chain 构建
- Agent 收到用户消息后：
  1. 结合聊天历史 + 可用 tools 构建 prompt
  2. 调用 LLM，获取回复（可能包含 tool calls）
  3. 如果有 tool call → 不直接执行，返回前端展示确认 UI
  4. 用户确认 → 执行 tool → 将结果反馈给 LLM → 继续对话
- 特殊处理：如果检测到删除意图 → 拒绝，建议移动到某文件夹

### 5.4 系统提示词
```
你是 PhotoMind，一个智能照片搜索助手。你可以：
1. 搜索照片（使用 search_photos tool）
2. 展示照片信息
3. 使用可用的 tools 帮助用户管理照片

规则：
- 永远不要删除照片。如果用户想删除，建议移动到一个文件夹让用户手动删除。
- 所有操作（移动、创建文件夹等）都需要用户确认。
- 搜索结果应显示照片缩略图、文件名、路径和基本信息。
```

---

## 阶段 6：Tool 系统

### 6.1 内置 Tools
- **search_photos**: 搜索照片（这个是 Agent 内部使用的，实际调用搜索系统）
- **move_file**: 移动照片到指定路径
- **create_folder**: 创建文件夹
- **get_photo_info**: 获取照片详细信息

### 6.2 外部 Tool 框架
- 用户可配置外部 tool，定义：
  - 名称、描述
  - 类型：HTTP API / CLI command
  - 参数 schema（JSON Schema）
  - 调用模板：
    - HTTP: method, url template, headers, body template
    - CLI: command template
- 示例：Immich 移动到相册
  ```json
  {
    "id": "external:immich:move_to_album",
    "name": "Move to Album (Immich)",
    "description": "Move a photo to an album in Immich",
    "category": "external",
    "config": {
      "type": "http",
      "method": "POST",
      "url": "http://immich:3001/api/album/{album_id}/assets",
      "headers": { "x-api-key": "{api_key}" },
      "body": { "ids": ["{asset_id}"] }
    },
    "schema": {
      "type": "object",
      "properties": {
        "album_id": { "type": "string", "description": "Album ID" },
        "asset_id": { "type": "string", "description": "Asset ID" }
      }
    }
  }
  ```

### 6.3 Tool 设置页
- 展示所有 tools（内置 + 外部），带搜索框
- 每个 tool 显示：名称、描述、类型、启用/禁用开关
- 外部 tool 的添加/编辑表单
- 通过 Agent 聊天配置的 tool 同样会出现在这里

---

## 阶段 7：前端实现

### 7.1 搜索页（首页）
- 大搜索框居中（类 Google 风格）
- 支持文字输入 + 图片上传按钮（以图搜图）
- 搜索结果：网格布局展示缩略图
- 点击照片 → 展开详情：大图、文件信息、路径、EXIF
- 快捷操作按钮（如果有 enabled tools）

### 7.2 聊天页
- 类似常见 AI 聊天界面
- 支持发送文字和图片
- Tool 调用确认 UI：Agent 要调用 tool 时，显示操作详情 + 确认/取消按钮
- 搜索结果内联展示（缩略图网格）

### 7.3 设置页
- **扫描目录**：添加/删除照片扫描目录，显示扫描状态
- **Embedding 模型**：Provider URL + API Key + 模型选择
- **Agent 模型**：Provider 选择 + URL + API Key + 模型选择/手动添加
- **Tools 管理**：搜索、浏览、启用/禁用、添加/编辑外部 tool
- **系统信息**：照片总数、已 embed 数、索引状态

---

## 阶段 8：Docker 部署

### 8.1 Dockerfile
- 多阶段构建：
  - Stage 1: Rust 编译（使用 cargo-chef 加速）
  - Stage 2: Node.js 构建前端
  - Stage 3: 最终镜像（debian-slim），只含二进制 + 静态资源

### 8.2 docker-compose.yml
```yaml
services:
  photomind:
    build: .
    ports:
      - "8080:8080"
    volumes:
      - /path/to/photos:/photos:ro    # 照片目录（只读）
      - photomind_data:/data           # SQLite + 缩略图缓存
    environment:
      - PHOTOMIND_DATA_DIR=/data
      - PHOTOMIND_PHOTO_DIRS=/photos
```

---

## 实施顺序（按任务拆分）

### Phase 1: 骨架搭建
- [x] **T01** Rust workspace 初始化（server, core, storage, tools crates）
- [x] **T02** React 前端初始化（Vite + React + TS + Tailwind + shadcn）
- [x] **T03** Axum HTTP server 基础框架 + 静态文件服务（serve React build）
- [x] **T04** SQLite 数据库初始化 + migration

### Phase 2: 数据基础
- [x] **T05** 照片扫描器（目录遍历、EXIF提取、hash计算、入库）
- [x] **T06** 缩略图生成与缓存
- [x] **T07** 扫描目录配置 API + 前端设置页（扫描部分）

### Phase 3: Embedding 与搜索
- [x] **T08** Embedding 模型配置（API调用层 + 设置页）
- [x] **T09** Embedding 流水线（队列处理、批量embed、入库）
- [x] **T10** 向量索引（内存加载 + 余弦相似度搜索）
- [x] **T11** 搜索 API + 前端搜索页

### Phase 4: Agent 系统
- [x] **T12** Agent Provider 抽象层（Anthropic/Google/OpenAI/OpenAI-compat）
- [x] **T13** Agent 模型配置（设置页 + 模型发现）
- [x] **T14** Agent 对话核心（消息处理、tool call 检测）
- [x] **T15** 聊天页前端（对话UI、图片发送、tool确认UI）

### Phase 5: Tool 系统
- [x] **T16** Tool 框架（注册、执行、确认机制）
- [x] **T17** 内置 tools 实现（search_photos, move_file, create_folder, get_photo_info）
- [x] **T18** 外部 tool 框架（HTTP/CLI 调用模板）
- [x] **T19** Tool 设置页前端（列表、搜索、添加/编辑、启用/禁用）
- [x] **T20** 删除意图拦截逻辑

### Phase 6: 完善与部署
- [x] **T21** 以图搜图（图片上传 + embedding + 搜索）
- [x] **T22** 文件系统监听（notify 实时更新）
- [x] **T23** Dockerfile + docker-compose.yml
- [x] **T24** 整体联调与错误处理完善
