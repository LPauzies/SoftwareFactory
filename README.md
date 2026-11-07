# WAGNER // Autonomous Software Factory

An agentic **Software Factory** written in **Rust (Axum + Tokio)** with a brutalist IDE **Vue 3 (TypeScript + Pinia + Vite)** UI that orchestrates **Gemini models** directly through **Antigravity (`agy`) subprocesses**, Linear project synchronization, and real-time Chain-of-Thought (CoT) human-in-the-loop steering.

---

## Core Capabilities

| Wagner Pillar | Implementation in this Repository |
| :--- | :--- |
| **Multi-Role Collaborative Design**<br>*(PO, Domain Expert, ARCHI, DEV, QA, OPS)* | 6 specialized Gemini agents (`AgentRole` in `backend/src/models.rs`) collaborate sequentially and pass structured context across design, architecture, implementation, verification, and release stages. |
| **Direct Antigravity (`agy`) Subprocess Bridge** | `AgyBridge` (`backend/src/agy.rs`) spawns `agy --print "<prompt>" --model <gemini-model> --effort <level> --add-dir <repo> --disable-slash-commands --output-format json` via `tokio::process::Command`. |
| **Real-Time Chain-of-Thought & Human Steering** | Inspect every agent's live `<thinking>` reasoning steps, tool probes, and `<question>` alerts in the UI drawer and steer or correct any agent mid-flight via `POST /api/tickets/:id/steer`. |
| **Linear Project & Ticket Kanban** | 5-column Kanban board (`Backlog / Todo`, `Design & Spec`, `Coding & SCM`, `Test Factory`, `Released & RUN`) synced with Linear (`https://api.linear.app/graphql`) and bound to local Git repositories. |
| **Modular Bricks ("Only enable the bricks you exploit")** | Dynamically enable or disable any of the 6 core Software Factory bricks (`project_management`, `scm_security`, `test_factory`, `living_documentation`, `continuous_deployment`, `run_and_rollback`). |
| **Connected Test Factory & Living Documentation** | Automatically generates executable Gherkin BDD `.feature` scenarios, Release Notes, Architecture Wiki pages, and pre-armed rollback scripts. |

---

## Plugging Your Linear Account into WAGNER

You can connect your Linear workspace to WAGNER in **two ways**:

### 1. Create a Linear Personal API Key
1. Open Linear and go to **Settings → Account → Security & Access → Personal API keys** (or visit `https://linear.app/settings/api`).
2. Click **Create key**, give it a label such as `WAGNER Software Factory`, and copy the generated token (`lin_api_...`).

### 2. Connect via the WAGNER UI (Interactive) or Environment Variable
- **Option A — Inside the UI (`http://localhost:8888`)**:
  1. Click the **`SYNC LINEAR`** button in the top navigation bar.
  2. Paste your `lin_api_...` token into the **LINEAR PERSONAL API KEY** input field and click **`SYNC LINEAR WORKSPACE`**.
  3. WAGNER will immediately query `https://api.linear.app/graphql`, import your Linear Projects and Issues into the Kanban board, and post automated release summaries back to your Linear issues when a pipeline run completes.
- **Option B — Environment Variable (`LINEAR_API_KEY`)**:
  Export `LINEAR_API_KEY=lin_api_...` before running `just` or `docker compose up --build -d`. When `LINEAR_API_KEY` is set in the environment, clicking **`SYNC LINEAR`** in the UI will sync immediately without needing to paste the key.

---

## Quick Start (`http://localhost:8888`)

Start the entire stack (Rust backend + Vue 3 frontend on `http://localhost:8888`) with a single command using [`justfile`](file:///home/mediaserver/Documents/SoftwareFactory/justfile):

```bash
just
```

Then open **`http://localhost:8888`**.

Other handy commands:
- `just test` — Run the Rust backend unit & integration test suite (`cargo test --all-targets`)
- `just prod` — Build the Vue 3 bundle and serve both UI + API from the single Rust binary on `http://localhost:8888`
- `just docker-up` — Build and launch the single-container Docker stack on `http://localhost:8888`

---

## Single-Container Docker Deployment (`http://localhost:8888`)

A multi-stage [`Dockerfile`](file:///home/mediaserver/Documents/SoftwareFactory/Dockerfile) builds both the Vue 3 production bundle and the Rust release binary (`wagner-backend`), serving the SPA and REST/SSE API together on port `8888`.

### Option A: Docker Compose (Recommended)

```bash
docker compose up --build -d
```

Then open **`http://localhost:8888`**.

### Option B: Standalone `docker build` & `docker run`

```bash
docker build -t wagner-software-factory .

docker run -d \
  --name wagner-software-factory \
  -p 8888:8888 \
  -e LINEAR_API_KEY="${LINEAR_API_KEY:-}" \
  -v "$(pwd)/data:/app/data" \
  -v "$(pwd)/workspaces:/app/workspaces" \
  -v "/home/mediaserver/.local/bin/agy:/usr/local/bin/agy:ro" \
  -v "$HOME/.gemini:/root/.gemini" \
  wagner-software-factory
```
