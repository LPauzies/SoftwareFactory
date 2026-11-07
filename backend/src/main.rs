mod agy;
mod config;
mod linear;
mod models;
mod orchestrator;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse,
    },
    routing::{delete, get, post, put},
    Json, Router,
};
use chrono::Utc;
use futures::stream::Stream;
use models::*;
use orchestrator::Orchestrator;
use serde::{Deserialize, Serialize};
use std::{convert::Infallible, net::SocketAddr, path::PathBuf, sync::Arc, time::Duration};
use tokio_stream::{wrappers::BroadcastStream, StreamExt};
use tower_http::{
    cors::{Any, CorsLayer},
    services::{ServeDir, ServeFile},
};
use tracing::info;
use uuid::Uuid;

#[derive(Deserialize)]
pub struct CreateProjectReq {
    pub name: String,
    pub description: String,
    pub domain: String,
}

#[derive(Deserialize)]
pub struct UpdateBricksReq {
    pub bricks: Vec<FactoryBrick>,
}

#[derive(Deserialize)]
pub struct UpdateAgentsReq {
    pub agents: Vec<AgentConfig>,
}

#[derive(Deserialize)]
pub struct StartRunReq {
    pub version_tag: String,
    pub feature_request: String,
    pub execution_mode: Option<String>,
}

#[derive(Deserialize)]
pub struct TriggerRollbackReq {
    pub ticket_id: Option<String>,
    pub target_release_id: Option<String>,
    pub reason: String,
    pub use_agy_live: Option<bool>,
}

#[derive(Deserialize)]
pub struct SyncLinearReq {
    pub api_key: Option<String>,
}

#[derive(Deserialize)]
pub struct UpdateLinearProjectRepoReq {
    pub default_repo_path: String,
    pub default_github_repo: Option<String>,
}

#[derive(Deserialize)]
pub struct CreateTicketReq {
    pub linear_project_id: String,
    pub title: String,
    pub description: String,
    pub priority_label: Option<String>,
    pub repository_path: Option<String>,
}

#[derive(Deserialize)]
pub struct LaunchTicketReq {
    pub repository_path: Option<String>,
    pub execution_mode: Option<String>, // "agy_live" | "agy_fast_simulation"
}

#[derive(Deserialize)]
pub struct SteerTicketAgentReq {
    pub role: AgentRole,
    pub message: String,
    pub execution_mode: Option<String>,
}

#[derive(Deserialize)]
pub struct MoveTicketStageReq {
    pub stage: KanbanStage,
}

#[derive(Deserialize)]
pub struct ConsultAgentReq {
    pub role: AgentRole,
    pub model: String,
    pub effort: Option<String>,
    pub prompt: String,
    pub conversation_id: Option<String>,
}

#[derive(Serialize)]
pub struct ConsultAgentResp {
    pub role: AgentRole,
    pub model: String,
    pub agy_command: String,
    pub conversation_id: Option<String>,
    pub response: String,
    pub duration_seconds: f64,
    pub usage: AgyUsage,
}

pub fn build_app_router(orchestrator: Arc<Orchestrator>, static_dir: PathBuf) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let index_file = static_dir.join("index.html");
    let spa_service = ServeDir::new(&static_dir).not_found_service(ServeFile::new(index_file));

    Router::new()
        .route("/api/health", get(health_handler))
        .route("/api/agy/status", get(agy_status_handler))
        .route("/api/state", get(get_state_handler))
        .route("/api/linear/sync", post(sync_linear_handler))
        .route(
            "/api/linear/projects/:id/repo",
            put(update_linear_project_repo_handler),
        )
        .route(
            "/api/project-mappings",
            get(get_project_mappings_handler).post(upsert_project_mapping_handler),
        )
        .route("/api/tickets", post(create_ticket_handler))
        .route("/api/tickets/:id", delete(delete_ticket_handler))
        .route("/api/tickets/:id/delete", post(delete_ticket_handler))
        .route("/api/tickets/:id/launch", post(launch_ticket_handler))
        .route("/api/tickets/:id/cancel", post(cancel_ticket_handler))
        .route("/api/tickets/:id/steer", post(steer_ticket_handler))
        .route("/api/tickets/:id/stage", put(move_ticket_stage_handler))
        .route("/api/projects", post(create_project_handler))
        .route("/api/projects/:id/bricks", put(update_bricks_handler))
        .route("/api/projects/:id/agents", put(update_agents_handler))
        .route(
            "/api/marketplace/:plugin_id/toggle",
            post(toggle_plugin_handler),
        )
        .route("/api/projects/:id/runs", post(start_run_handler))
        .route("/api/projects/:id/rollback", post(rollback_handler))
        .route("/api/agy/consult", post(consult_agent_handler))
        .route("/api/events", get(sse_handler))
        .fallback_service(spa_service)
        .layer(cors)
        .with_state(orchestrator)
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            std::env::var("RUST_LOG")
                .unwrap_or_else(|_| "software_factory_backend=info,tower_http=info".to_string()),
        )
        .init();

    let root_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let data_dir = root_dir.join("data");
    let default_repo_dir = PathBuf::from("/home/mediaserver/Documents/SoftwareFactory");

    let orchestrator = Arc::new(Orchestrator::new(data_dir, default_repo_dir));

    // Ensure any completed tickets have their local and remote Git branches created and pushed on startup
    let orch_pr_init = Arc::clone(&orchestrator);
    tokio::spawn(async move {
        orch_pr_init.ensure_all_pr_branches_pushed().await;
    });

    // Automatically sync Linear on startup and every `linear_sync_interval_seconds`,
    // and continuously monitor for merged PRs & auto-archive cards (CAI-25)
    let orch_bg = Arc::clone(&orchestrator);
    tokio::spawn(async move {
        loop {
            orch_bg.check_merged_prs_and_auto_archive().await;
            let cfg = config::WagnerConfig::load_from_path(&orch_bg.config_file);
            let has_key = cfg.linear_api_key.is_some() || orch_bg.linear_api_key.read().await.is_some();
            if has_key {
                let _ = orch_bg.sync_linear(None).await;
            }
            tokio::time::sleep(Duration::from_secs(10)).await;
        }
    });

    let static_dir = std::env::var("STATIC_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let docker_static = root_dir.join("static");
            if docker_static.exists() {
                docker_static
            } else {
                root_dir.join("../frontend/dist")
            }
        });

    let app = build_app_router(orchestrator, static_dir);

    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(3001);
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    info!("WAGNER Gemini Software Factory backend listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn health_handler() -> impl IntoResponse {
    Json(serde_json::json!({
        "status": "ok",
        "service": "wagner-gemini-software-factory",
        "engine": "antigravity-subprocess"
    }))
}

async fn agy_status_handler(State(orch): State<Arc<Orchestrator>>) -> impl IntoResponse {
    let status = orch.agy.check_status().await;
    Json(status)
}

async fn get_state_handler(State(orch): State<Arc<Orchestrator>>) -> impl IntoResponse {
    let guard = orch.state.read().await;
    Json(guard.clone())
}

async fn sync_linear_handler(
    State(orch): State<Arc<Orchestrator>>,
    Json(req): Json<SyncLinearReq>,
) -> Result<Json<LinearStatus>, (StatusCode, String)> {
    orch.check_merged_prs_and_auto_archive().await;
    match orch.sync_linear(req.api_key).await {
        Ok(status) => Ok(Json(status)),
        Err(e) => Err((StatusCode::BAD_REQUEST, e.to_string())),
    }
}

async fn get_project_mappings_handler(
    State(orch): State<Arc<Orchestrator>>,
) -> Json<Vec<config::ProjectRepoMapping>> {
    let cfg = config::WagnerConfig::load_from_path(&orch.config_file);
    Json(cfg.project_mappings)
}

async fn upsert_project_mapping_handler(
    State(orch): State<Arc<Orchestrator>>,
    Json(req): Json<config::ProjectRepoMapping>,
) -> Result<Json<Vec<config::ProjectRepoMapping>>, (StatusCode, String)> {
    if req.linear_project.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "linear_project cannot be empty".to_string(),
        ));
    }
    config::WagnerConfig::upsert_project_mapping(&orch.config_file, &req);
    let cfg = config::WagnerConfig::load_from_path(&orch.config_file);

    // Apply updated mapping to any matching LinearProject and its tickets in memory
    {
        let mut guard = orch.state.write().await;
        for lp in guard.linear_projects.iter_mut() {
            if let Some(m) = cfg.find_mapping_for_project(&lp.name, &lp.key, &lp.id) {
                if !m.local_path.is_empty() {
                    lp.default_repo_path = m.local_path.clone();
                }
                if !m.github_repo.is_empty() {
                    lp.default_github_repo = m.github_repo.clone();
                }
            }
        }
        let lp_snapshot = guard.linear_projects.clone();
        for tk in guard.tickets.iter_mut() {
            if let Some(proj) = lp_snapshot.iter().find(|p| {
                p.id == tk.linear_project_id || tk.linear_team_id.as_deref() == Some(&p.id)
            }) {
                if !proj.default_repo_path.is_empty() {
                    tk.repository_path = proj.default_repo_path.clone();
                }
                if !proj.default_github_repo.is_empty() {
                    tk.github_repo = proj.default_github_repo.clone();
                }
                if tk.stage == KanbanStage::PrReview {
                    if let Some(ref br) = tk.pr_branch {
                        let pr_title =
                            Orchestrator::format_pr_title(Some(&tk.identifier), &tk.title);
                        tk.pr_url = Some(Orchestrator::build_github_compare_url(
                            &tk.github_repo,
                            "master",
                            br,
                            &pr_title,
                        ));
                    }
                }
            }
        }
    }
    orch.persist().await;
    Ok(Json(cfg.project_mappings))
}

async fn update_linear_project_repo_handler(
    State(orch): State<Arc<Orchestrator>>,
    Path(id): Path<String>,
    Json(req): Json<UpdateLinearProjectRepoReq>,
) -> Result<Json<LinearProject>, (StatusCode, String)> {
    let repo_path = req.default_repo_path.trim().to_string();
    let _ = std::fs::create_dir_all(&repo_path);
    let cfg = config::WagnerConfig::load_from_path(&orch.config_file);
    let gh_repo = req
        .default_github_repo
        .map(|g| g.trim().to_string())
        .filter(|g| !g.is_empty())
        .unwrap_or_else(|| Orchestrator::detect_github_owner_repo(&PathBuf::from(&repo_path), &cfg));

    let updated = {
        let mut guard = orch.state.write().await;
        let lp = guard
            .linear_projects
            .iter_mut()
            .find(|p| p.id == id)
            .ok_or((StatusCode::NOT_FOUND, "Linear project not found".to_string()))?;
        lp.default_repo_path = repo_path.clone();
        lp.default_github_repo = gh_repo.clone();
        let lp_clone = lp.clone();

        for tk in guard.tickets.iter_mut() {
            if tk.linear_project_id == id || tk.linear_team_id.as_deref() == Some(&id) {
                tk.repository_path = repo_path.clone();
                tk.github_repo = gh_repo.clone();
                if tk.stage == KanbanStage::PrReview {
                    if let Some(ref br) = tk.pr_branch {
                        let pr_title =
                            Orchestrator::format_pr_title(Some(&tk.identifier), &tk.title);
                        tk.pr_url = Some(Orchestrator::build_github_compare_url(
                            &gh_repo,
                            "master",
                            br,
                            &pr_title,
                        ));
                    }
                }
            }
        }
        lp_clone
    };

    // Persist the 1:1:1 binding into `wagner.toml` `[[projects]]`
    config::WagnerConfig::upsert_project_mapping(
        &orch.config_file,
        &config::ProjectRepoMapping {
            linear_project: updated.name.clone(),
            local_path: repo_path,
            github_repo: gh_repo,
            default_branch: "master".to_string(),
        },
    );

    orch.persist().await;
    Ok(Json(updated))
}

async fn create_ticket_handler(
    State(orch): State<Arc<Orchestrator>>,
    Json(req): Json<CreateTicketReq>,
) -> Result<(StatusCode, Json<FactoryTicket>), (StatusCode, String)> {
    let ticket = {
        let mut guard = orch.state.write().await;
        let lp = guard
            .linear_projects
            .iter()
            .find(|p| p.id == req.linear_project_id)
            .cloned()
            .ok_or((StatusCode::NOT_FOUND, "Linear project not found".to_string()))?;

        let count = guard
            .tickets
            .iter()
            .filter(|t| t.linear_project_id == lp.id)
            .count()
            + 101;
        let identifier = format!("{}-{}", lp.key, count);
        let repo = req
            .repository_path
            .filter(|r| !r.trim().is_empty())
            .unwrap_or_else(|| lp.default_repo_path.clone());
        let gh_repo = if !lp.default_github_repo.is_empty() {
            lp.default_github_repo.clone()
        } else {
            "LPauzies/SoftwareFactory".to_string()
        };

        let new_ticket = FactoryTicket {
            id: format!("ticket-{}", &Uuid::new_v4().to_string()[..8]),
            identifier,
            title: req.title,
            description: req.description,
            priority_label: req.priority_label.unwrap_or_else(|| "High".to_string()),
            linear_project_id: lp.id,
            linear_team_id: None,
            linear_url: None,
            repository_path: repo,
            github_repo: gh_repo,
            stage: KanbanStage::Todo,
            active_role: None,
            active_run_id: None,
            pending_question: None,
            pr_url: None,
            pr_branch: None,
            version_tag: format!("v1.{}.0", guard.tickets.len() + 1),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            released_at: None,
            archived: false,
        };
        guard.tickets.insert(0, new_ticket.clone());
        new_ticket
    };
    orch.persist().await;
    Ok((StatusCode::CREATED, Json(ticket)))
}

async fn launch_ticket_handler(
    State(orch): State<Arc<Orchestrator>>,
    Path(id): Path<String>,
    Json(req): Json<LaunchTicketReq>,
) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, String)> {
    let mode = req
        .execution_mode
        .unwrap_or_else(|| "agy_live".to_string());
    match Arc::clone(&orch)
        .launch_ticket_factory(id, req.repository_path, mode)
        .await
    {
        Ok((ticket, run)) => Ok((
            StatusCode::ACCEPTED,
            Json(serde_json::json!({
                "ticket": ticket,
                "run": run
            })),
        )),
        Err(e) => Err((StatusCode::BAD_REQUEST, e.to_string())),
    }
}

async fn cancel_ticket_handler(
    State(orch): State<Arc<Orchestrator>>,
    Path(id): Path<String>,
) -> Result<Json<FactoryTicket>, (StatusCode, String)> {
    match orch.cancel_ticket_agent(&id).await {
        Ok(ticket) => Ok(Json(ticket)),
        Err(e) => Err((StatusCode::BAD_REQUEST, e.to_string())),
    }
}

async fn delete_ticket_handler(
    State(orch): State<Arc<Orchestrator>>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    match orch.delete_ticket(&id).await {
        Ok(identifier) => Ok(Json(serde_json::json!({
            "deleted": true,
            "ticket_id": id,
            "identifier": identifier
        }))),
        Err(e) => Err((StatusCode::NOT_FOUND, e.to_string())),
    }
}

async fn steer_ticket_handler(
    State(orch): State<Arc<Orchestrator>>,
    Path(id): Path<String>,
    Json(req): Json<SteerTicketAgentReq>,
) -> Result<Json<AgentStepExecution>, (StatusCode, String)> {
    let mode = req
        .execution_mode
        .unwrap_or_else(|| "agy_live".to_string());
    match orch
        .steer_ticket_agent(&id, req.role, &req.message, &mode)
        .await
    {
        Ok(step) => Ok(Json(step)),
        Err(e) => Err((StatusCode::BAD_REQUEST, e.to_string())),
    }
}

async fn move_ticket_stage_handler(
    State(orch): State<Arc<Orchestrator>>,
    Path(id): Path<String>,
    Json(req): Json<MoveTicketStageReq>,
) -> Result<Json<FactoryTicket>, (StatusCode, String)> {
    let updated = {
        let mut guard = orch.state.write().await;
        let tk = guard
            .tickets
            .iter_mut()
            .find(|t| t.id == id)
            .ok_or((StatusCode::NOT_FOUND, "Ticket not found".to_string()))?;
        tk.stage = req.stage;
        if req.stage == KanbanStage::Todo
            || req.stage == KanbanStage::Done
            || req.stage == KanbanStage::PrReview
        {
            tk.active_role = None;
        }
        if req.stage == KanbanStage::Done && tk.released_at.is_none() {
            tk.released_at = Some(Utc::now());
        }
        tk.updated_at = Utc::now();
        tk.clone()
    };
    orch.persist().await;
    orch.sync_ticket_stage_to_linear(&updated.id, updated.stage).await;
    let _ = orch.event_tx.send(FactoryEvent::TicketUpdated {
        ticket: updated.clone(),
    });
    Ok(Json(updated))
}

async fn create_project_handler(
    State(orch): State<Arc<Orchestrator>>,
    Json(req): Json<CreateProjectReq>,
) -> impl IntoResponse {
    let slug: String = req
        .name
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .trim_matches('-')
        .to_string();
    let repo_path = orch.workspaces_root.join(&slug).to_string_lossy().to_string();
    let _ = std::fs::create_dir_all(&repo_path);

    let project = Project {
        id: format!("proj-{}", &Uuid::new_v4().to_string()[..8]),
        name: req.name,
        slug,
        description: req.description,
        domain: req.domain,
        repository_path: repo_path,
        active_version: "v0.1.0".to_string(),
        created_at: Utc::now(),
        bricks: Orchestrator::default_bricks(),
        agents: Orchestrator::default_agents(),
        homogeneity_score: 96,
    };

    {
        let mut guard = orch.state.write().await;
        guard.projects.insert(0, project.clone());
    }
    orch.persist().await;
    (StatusCode::CREATED, Json(project))
}

async fn update_bricks_handler(
    State(orch): State<Arc<Orchestrator>>,
    Path(id): Path<String>,
    Json(req): Json<UpdateBricksReq>,
) -> Result<Json<Project>, (StatusCode, String)> {
    let updated = {
        let mut guard = orch.state.write().await;
        let proj = guard
            .projects
            .iter_mut()
            .find(|p| p.id == id)
            .ok_or((StatusCode::NOT_FOUND, "Project not found".to_string()))?;
        proj.bricks = req.bricks;
        let enabled_count = proj.bricks.iter().filter(|b| b.enabled).count();
        proj.homogeneity_score = (88 + (enabled_count * 2).min(11)) as u8;
        proj.clone()
    };
    orch.persist().await;
    Ok(Json(updated))
}

async fn update_agents_handler(
    State(orch): State<Arc<Orchestrator>>,
    Path(id): Path<String>,
    Json(req): Json<UpdateAgentsReq>,
) -> Result<Json<Project>, (StatusCode, String)> {
    let updated = {
        let mut guard = orch.state.write().await;
        let proj = guard
            .projects
            .iter_mut()
            .find(|p| p.id == id)
            .ok_or((StatusCode::NOT_FOUND, "Project not found".to_string()))?;
        proj.agents = req
            .agents
            .into_iter()
            .map(|mut a| {
                a.system_prompt = Orchestrator::sanitize_agent_definition_text(&a.system_prompt);
                a.title = Orchestrator::sanitize_agent_definition_text(&a.title);
                a
            })
            .collect();
        proj.clone()
    };
    orch.persist().await;
    Ok(Json(updated))
}

async fn toggle_plugin_handler(
    State(orch): State<Arc<Orchestrator>>,
    Path(plugin_id): Path<String>,
) -> Result<Json<MarketplacePlugin>, (StatusCode, String)> {
    let updated = {
        let mut guard = orch.state.write().await;
        let plugin = guard
            .marketplace
            .iter_mut()
            .find(|m| m.id == plugin_id)
            .ok_or((StatusCode::NOT_FOUND, "Plugin not found".to_string()))?;
        plugin.installed = !plugin.installed;
        plugin.clone()
    };
    orch.persist().await;
    Ok(Json(updated))
}

async fn start_run_handler(
    State(orch): State<Arc<Orchestrator>>,
    Path(id): Path<String>,
    Json(req): Json<StartRunReq>,
) -> Result<(StatusCode, Json<PipelineRun>), (StatusCode, String)> {
    let mode = req
        .execution_mode
        .unwrap_or_else(|| "agy_live".to_string());
    match Arc::clone(&orch)
        .start_pipeline_run(id, req.version_tag, req.feature_request, mode)
        .await
    {
        Ok(run) => Ok((StatusCode::ACCEPTED, Json(run))),
        Err(e) => Err((StatusCode::BAD_REQUEST, e.to_string())),
    }
}

async fn rollback_handler(
    State(orch): State<Arc<Orchestrator>>,
    Path(id): Path<String>,
    Json(req): Json<TriggerRollbackReq>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let use_live = req.use_agy_live.unwrap_or(true);
    match orch
        .trigger_rollback(
            &id,
            req.ticket_id,
            req.target_release_id,
            &req.reason,
            use_live,
        )
        .await
    {
        Ok((rollback, ticket, restored)) => Ok(Json(serde_json::json!({
            "rollback": rollback,
            "ticket": ticket,
            "restored_release": restored
        }))),
        Err(e) => Err((StatusCode::BAD_REQUEST, e.to_string())),
    }
}

async fn consult_agent_handler(
    State(orch): State<Arc<Orchestrator>>,
    Json(req): Json<ConsultAgentReq>,
) -> Result<Json<ConsultAgentResp>, (StatusCode, String)> {
    let effort = req.effort.unwrap_or_else(|| "low".to_string());
    let agy_cmd = orch
        .agy
        .format_cli_command(&req.model, &effort, req.role.code());

    let full_prompt = format!(
        "You are acting as {} ({}) in the WAGNER Software Factory ({}). \
        Do NOT call any tools or read/write files. Answer the following request concisely in Markdown:\n\n{}",
        req.role.title(),
        req.role.code(),
        req.role.wagner_pillar(),
        req.prompt
    );

    let start = std::time::Instant::now();
    match orch
        .agy
        .run_agent_prompt(
            &req.model,
            &effort,
            &full_prompt,
            None,
            req.conversation_id.as_deref(),
        )
        .await
    {
        Ok(resp) => Ok(Json(ConsultAgentResp {
            role: req.role,
            model: req.model,
            agy_command: agy_cmd,
            conversation_id: resp.conversation_id,
            response: resp
                .response
                .unwrap_or_else(|| "No response text returned.".to_string()),
            duration_seconds: resp
                .duration_seconds
                .unwrap_or_else(|| start.elapsed().as_secs_f64()),
            usage: resp.usage.unwrap_or_default().into(),
        })),
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
    }
}

async fn sse_handler(
    State(orch): State<Arc<Orchestrator>>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let rx = orch.event_tx.subscribe();
    let stream = BroadcastStream::new(rx).filter_map(|msg| match msg {
        Ok(evt) => {
            let json = serde_json::to_string(&evt).unwrap_or_default();
            Some(Ok(Event::default().data(json)))
        }
        Err(_) => None,
    });

    Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(10))
            .text("keep-alive"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agy::AgyBridge;
    use std::env;

    fn create_test_orchestrator(test_name: &str) -> Arc<Orchestrator> {
        let base = env::temp_dir().join(format!("wagner_test_{}_{}", test_name, Uuid::new_v4()));
        let data_dir = base.join("data");
        let workspaces_dir = base.join("workspaces");
        Arc::new(Orchestrator::new(data_dir, workspaces_dir))
    }

    #[tokio::test]
    async fn test_initial_seed_has_six_roles_six_bricks_and_linear_projects() {
        let orch = create_test_orchestrator("seed");
        let state = orch.state.read().await;

        assert!(!state.projects.is_empty(), "Should seed default Wagner project");
        assert_eq!(state.projects[0].bricks.len(), 6, "Must have 6 Wagner bricks");
        assert_eq!(state.projects[0].agents.len(), 7, "Must have 7 agent roles (including PR)");
        assert!(
            state.linear_projects.len() >= 3,
            "Should seed 3 Linear projects"
        );
        assert!(
            state.tickets.len() >= 5,
            "Should seed Kanban tickets across stages"
        );
    }

    #[tokio::test]
    async fn test_extract_thoughts_and_question_from_agent_output() {
        let raw = "<thinking>\n- Evaluating Redis lock TTL invariants\n- Checking rollback safety\n</thinking>\n<question>Should lock TTL default to 15s or 30s?</question>\n### Deliverable\nLock spec ready.";
        let (thoughts, question, cleaned) =
            AgyBridge::extract_thoughts_and_clean_response(AgentRole::Archi, raw, None);

        assert_eq!(thoughts.len(), 3); // 2 reasoning + 1 question
        assert_eq!(thoughts[0].kind, "reasoning");
        assert_eq!(
            question.as_deref(),
            Some("Should lock TTL default to 15s or 30s?")
        );
        assert_eq!(cleaned, "### Deliverable\nLock spec ready.");
    }

    #[tokio::test]
    async fn test_launch_ticket_and_human_in_the_loop_steering() {
        let orch = create_test_orchestrator("launch_and_steer");
        let ticket_id = {
            let guard = orch.state.read().await;
            guard
                .tickets
                .iter()
                .find(|t| t.stage == KanbanStage::Todo)
                .expect("Should have a Todo ticket")
                .id
                .clone()
        };

        let (launched_ticket, run) = Arc::clone(&orch)
            .launch_ticket_factory(
                ticket_id.clone(),
                None,
                "agy_fast_simulation".to_string(),
            )
            .await
            .expect("Launch should succeed");

        assert_eq!(launched_ticket.stage, KanbanStage::Design);
        assert_eq!(run.steps.len(), 7);

        // Steer the PO agent with human operator input
        let steered_step = orch
            .steer_ticket_agent(
                &ticket_id,
                AgentRole::Po,
                "Enforce a strict 20s timeout and idempotent retry header",
                "agy_fast_simulation",
            )
            .await
            .expect("Steering should succeed");

        assert!(
            steered_step
                .thinking_log
                .iter()
                .any(|t| t.kind == "user_steering" && t.content.contains("20s timeout")),
            "Chain of thought must record the human steering intervention"
        );
        assert!(
            steered_step.markdown_output.contains("Operator Steering Applied"),
            "Deliverable must incorporate operator guidance"
        );
    }

    #[tokio::test]
    async fn test_rollback_restores_previous_release_and_marks_ticket() {
        let orch = create_test_orchestrator("rollback");
        let proj_id = orch.state.read().await.projects[0].id.clone();

        let (rb_event, affected_ticket, restored_rel) = orch
            .trigger_rollback(
                &proj_id,
                Some("ticket-wag-102".to_string()),
                None,
                "Regression detected in latency probe",
                false,
            )
            .await
            .expect("Rollback should succeed");

        assert_eq!(rb_event.from_version, "v1.1.0");
        assert_eq!(rb_event.to_version, "v1.0.0");
        assert_eq!(
            affected_ticket.expect("Ticket should be updated").stage,
            KanbanStage::RolledBack
        );
        assert_eq!(
            restored_rel.expect("Previous release restored").version,
            "v1.0.0"
        );
    }

    #[tokio::test]
    async fn test_disabled_brick_skips_corresponding_agent_role() {
        let orch = create_test_orchestrator("disabled_brick");
        let proj_id = {
            let mut guard = orch.state.write().await;
            let p = &mut guard.projects[0];
            // Disable the TestFactory brick ("Ne configurez pas les briques que vous n'allez pas exploiter")
            for b in p.bricks.iter_mut() {
                if b.kind == BrickKind::TestFactory {
                    b.enabled = false;
                }
            }
            p.id.clone()
        };

        let run = Arc::clone(&orch)
            .start_pipeline_run(
                proj_id,
                "v2.0.0".to_string(),
                "Lean feature without Test Factory brick".to_string(),
                "agy_fast_simulation".to_string(),
            )
            .await
            .expect("Run should start");

        let qa_step = run
            .steps
            .iter()
            .find(|s| s.role == AgentRole::Qa)
            .expect("QA step exists");
        assert_eq!(
            qa_step.status,
            StepStatus::Skipped,
            "QA role must be skipped when TestFactory brick is disabled"
        );
    }

    #[tokio::test]
    async fn test_kill_running_agent_aborts_task_and_resets_ticket() {
        let orch = create_test_orchestrator("kill_agent");
        let ticket_id = {
            let guard = orch.state.read().await;
            guard
                .tickets
                .iter()
                .find(|t| t.stage == KanbanStage::Todo)
                .expect("Should have a Todo ticket")
                .id
                .clone()
        };

        let (launched, _run) = Arc::clone(&orch)
            .launch_ticket_factory(
                ticket_id.clone(),
                None,
                "agy_fast_simulation".to_string(),
            )
            .await
            .expect("Launch should succeed");

        assert_eq!(launched.stage, KanbanStage::Design);

        let cancelled = orch
            .cancel_ticket_agent(&ticket_id)
            .await
            .expect("Cancel should succeed");

        assert_eq!(cancelled.stage, KanbanStage::Todo);
        assert!(cancelled.active_role.is_none());
        assert!(cancelled.pending_question.is_none());
    }

    #[test]
    fn test_wagner_toml_config_loads_and_saves_secrets() {
        let tmp_toml = env::temp_dir().join(format!("wagner_cfg_{}.toml", Uuid::new_v4()));
        let initial = r#"# Wagner Config
[secrets]
linear_api_key = ""
github_token = "ghp_test_token_123"
custom_webhook_secret = "whsec_abc"

[factory]
linear_sync_interval_seconds = 15
default_repository_path = "/tmp/my-repo"
"#;
        std::fs::write(&tmp_toml, initial).expect("Should write temp toml");

        let loaded = config::WagnerConfig::load_from_path(&tmp_toml);
        assert!(loaded.linear_api_key.is_none());
        assert_eq!(loaded.github_token.as_deref(), Some("ghp_test_token_123"));
        assert_eq!(
            loaded.extra_secrets.get("custom_webhook_secret").map(|s| s.as_str()),
            Some("whsec_abc")
        );
        assert_eq!(loaded.linear_sync_interval_seconds, 15);
        assert_eq!(loaded.default_repository_path.as_deref(), Some("/tmp/my-repo"));

        // Save Linear API key and reload
        config::WagnerConfig::save_linear_api_key(&tmp_toml, "lin_api_secret_999");
        config::WagnerConfig::upsert_project_mapping(
            &tmp_toml,
            &config::ProjectRepoMapping {
                linear_project: "Split Second".to_string(),
                local_path: "/home/mediaserver/Documents/SplitSecond".to_string(),
                github_repo: "LPauzies/SplitSecond".to_string(),
                default_branch: "master".to_string(),
            },
        );
        let reloaded = config::WagnerConfig::load_from_path(&tmp_toml);
        assert_eq!(reloaded.linear_api_key.as_deref(), Some("lin_api_secret_999"));
        assert_eq!(reloaded.github_token.as_deref(), Some("ghp_test_token_123"));
        let found = reloaded
            .find_mapping_for_project("Split Second", "SPLI", "proj-1")
            .expect("Should find Split Second mapping");
        assert_eq!(found.local_path, "/home/mediaserver/Documents/SplitSecond");
        assert_eq!(found.github_repo, "LPauzies/SplitSecond");

        let _ = std::fs::remove_file(&tmp_toml);
    }
}
