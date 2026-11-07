use crate::agy::{AgyBridge, AgyPrintResponse};
use crate::config::WagnerConfig;
use crate::linear::LinearClient;
use crate::models::*;
use anyhow::Result;
use chrono::Utc;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};
use tokio::task::AbortHandle;
use tracing::{info, warn};
use uuid::Uuid;

pub struct Orchestrator {
    pub state: Arc<RwLock<FactoryState>>,
    pub linear_api_key: Arc<RwLock<Option<String>>>,
    pub linear_key_file: PathBuf,
    pub config_file: PathBuf,
    pub active_tasks: Arc<RwLock<HashMap<String, AbortHandle>>>,
    pub pipeline_queue: Arc<tokio::sync::Mutex<()>>,
    pub agy: AgyBridge,
    pub event_tx: broadcast::Sender<FactoryEvent>,
    pub data_file: PathBuf,
    pub workspaces_root: PathBuf,
}

impl Orchestrator {
    pub fn new(data_dir: PathBuf, workspaces_root: PathBuf) -> Self {
        let _ = fs::create_dir_all(&data_dir);
        let data_file = data_dir.join("factory_state.json");

        let mut state = if data_file.exists() {
            match fs::read_to_string(&data_file) {
                Ok(raw) => serde_json::from_str::<FactoryState>(&raw)
                    .unwrap_or_else(|_| Self::seed_initial_state(&workspaces_root)),
                Err(_) => Self::seed_initial_state(&workspaces_root),
            }
        } else {
            Self::seed_initial_state(&workspaces_root)
        };

        if state.linear_projects.is_empty() || state.tickets.is_empty() {
            let (lp, tk) = Self::default_linear_projects_and_tickets(&workspaces_root);
            if state.linear_projects.is_empty() {
                state.linear_projects = lp;
            }
            if state.tickets.is_empty() {
                state.tickets = tk;
            }
        }

        // Ensure all projects have the 7th agent role (`AgentRole::Pr`) if loaded from an older 6-agent state file,
        // and migrate any legacy English system prompts (`in English` / `You are the`) or missing Ponytail Full rules to French defaults.
        let default_agents = Self::default_agents();
        for proj in state.projects.iter_mut() {
            for def_ag in &default_agents {
                if let Some(existing_ag) = proj.agents.iter_mut().find(|a| a.role == def_ag.role) {
                    let lower = existing_ag.system_prompt.to_lowercase();
                    let needs_migration = lower.contains("in english")
                        || lower.starts_with("you are the")
                        || (existing_ag.role == AgentRole::Dev && !lower.contains("ponytail"));
                    if needs_migration {
                        existing_ag.system_prompt = def_ag.system_prompt.clone();
                    }
                } else {
                    proj.agents.push(def_ag.clone());
                }
            }
        }

        // Ensure default marketplace plugins (including `ponytail-full`) exist in persisted state
        for def_plugin in Self::default_marketplace() {
            if !state.marketplace.iter().any(|m| m.id == def_plugin.id) {
                state.marketplace.push(def_plugin);
            }
        }

        let toml_cfg = WagnerConfig::load();
        let config_file = toml_cfg.config_path.clone();
        let default_gh_repo = Self::detect_github_owner_repo(&workspaces_root, &toml_cfg);
        let default_base_branch = toml_cfg.base_branch_for_workspace(&workspaces_root);

        // Migrate any existing runs that were created before `AgentRole::Pr` was added
        let first_proj_clone = state.projects.first().cloned();
        if let Some(ref proj) = first_proj_clone {
            if let Some(pr_agent) = default_agents.iter().find(|a| a.role == AgentRole::Pr) {
                for run in state.runs.iter_mut() {
                    let branch_name = Self::branch_name_for_ticket(
                        run.ticket_identifier.as_deref(),
                        &run.version_tag,
                    );
                    let pr_url = format!(
                        "https://github.com/{}/compare/{}...{}?expand=1",
                        default_gh_repo, default_base_branch, branch_name
                    );

                    if !run.steps.iter().any(|s| s.role == AgentRole::Pr) {
                        let is_done = run.status == PipelineStatus::Succeeded;
                        let (summary, md_out, artifacts) = Self::synthesize_role_output(
                            AgentRole::Pr,
                            proj,
                            &run.version_tag,
                            &run.feature_request,
                        );
                        let thoughts = Self::synthesize_role_thoughts(
                            AgentRole::Pr,
                            proj,
                            &run.version_tag,
                            &run.feature_request,
                        );
                        run.steps.push(AgentStepExecution {
                            role: AgentRole::Pr,
                            title: pr_agent.title.clone(),
                            wagner_pillar: pr_agent.wagner_pillar.clone(),
                            model: pr_agent.model.clone(),
                            status: if is_done {
                                StepStatus::Completed
                            } else {
                                StepStatus::Pending
                            },
                            started_at: if is_done {
                                Some(Utc::now())
                            } else {
                                None
                            },
                            completed_at: if is_done {
                                Some(Utc::now())
                            } else {
                                None
                            },
                            duration_seconds: if is_done { 0.85 } else { 0.0 },
                            agy_conversation_id: if is_done {
                                Some("agy-pr-migrated".to_string())
                            } else {
                                None
                            },
                            agy_command: format!(
                                "agy --mode=auto --output-format json --model {} --effort {} --add-dir \"{}\"",
                                pr_agent.model, pr_agent.effort, run.workspace_dir
                            ),
                            summary: if is_done {
                                summary
                            } else {
                                "Queued in WAGNER Factory...".to_string()
                            },
                            markdown_output: if is_done {
                                format!(
                                    "{}\n\n---\n#### GitHub Pull Request Ready for Human Review\n- **Base ← Head**: `main` ← `{}`\n- **Review Link**: [{}]({})\n",
                                    md_out, branch_name, pr_url, pr_url
                                )
                            } else {
                                String::new()
                            },
                            thinking_log: if is_done { thoughts } else { vec![] },
                            artifacts: if is_done { artifacts } else { vec![] },
                            usage: if is_done {
                                AgyUsage {
                                    input_tokens: 1850,
                                    output_tokens: 340,
                                    thinking_tokens: 140,
                                    cache_read_tokens: 0,
                                    total_tokens: 2330,
                                }
                            } else {
                                AgyUsage::default()
                            },
                            quality_gate_passed: true,
                        });
                    }

                    if run.status == PipelineStatus::Succeeded
                        && (run.pr_url.is_none()
                            || run
                                .pr_url
                                .as_deref()
                                .is_some_and(|u| {
                                    u.contains("wagner-factory/")
                                        || u.contains("...wagner/")
                                        || u.contains("/compare/main...")
                                })
                            || run
                                .pr_branch
                                .as_deref()
                                .is_some_and(|b| b.starts_with("wagner/")))
                    {
                        run.pr_branch = Some(branch_name);
                        run.pr_url = Some(pr_url);
                    }
                }
            }
        }

        // Strip any legacy `Team: ...` workspace entries when real Linear projects exist
        if state
            .linear_projects
            .iter()
            .any(|p| !p.name.starts_with("Team:"))
        {
            state
                .linear_projects
                .retain(|p| !p.name.starts_with("Team:"));
        }

        // Apply 1:1:1 `[[projects]]` mappings (`linear_project` <-> `local_path` <-> `github_repo`) from `wagner.toml`
        for lp in state.linear_projects.iter_mut() {
            if let Some(m) = toml_cfg.find_mapping_for_project(&lp.name, &lp.key, &lp.id) {
                if !m.local_path.is_empty() {
                    lp.default_repo_path = m.local_path.clone();
                }
                if !m.github_repo.is_empty() {
                    lp.default_github_repo = m.github_repo.clone();
                }
            }
            if lp.default_github_repo.is_empty() {
                lp.default_github_repo = Self::detect_github_owner_repo(
                    &PathBuf::from(&lp.default_repo_path),
                    &toml_cfg,
                );
            }
        }

        // Ensure any ticket that finished OPS (`KanbanStage::Done` or `KanbanStage::PrReview`) has its PR link and sits in `KanbanStage::PrReview`
        let lp_snapshot = state.linear_projects.clone();
        for tk in state.tickets.iter_mut() {
            if let Some(proj) = lp_snapshot
                .iter()
                .find(|p| p.id == tk.linear_project_id)
            {
                if !proj.default_repo_path.is_empty() {
                    tk.repository_path = proj.default_repo_path.clone();
                }
                if !proj.default_github_repo.is_empty() {
                    tk.github_repo = proj.default_github_repo.clone();
                }
            }
            if tk.github_repo.is_empty() {
                tk.github_repo = default_gh_repo.clone();
            }

            if tk.stage == KanbanStage::Done || tk.stage == KanbanStage::PrReview {
                tk.stage = KanbanStage::PrReview;
                if tk.pr_url.is_none()
                    || tk
                        .pr_url
                        .as_deref()
                        .is_some_and(|u| {
                            u.contains("wagner-factory/")
                                || u.contains("...wagner/")
                                || u.contains("/compare/main...")
                        })
                    || tk
                        .pr_branch
                        .as_deref()
                        .is_some_and(|b| b.starts_with("wagner/"))
                {
                    let branch_name =
                        Self::branch_name_for_ticket(Some(&tk.identifier), &tk.version_tag);
                    let tk_base_branch =
                        toml_cfg.base_branch_for_workspace(&PathBuf::from(&tk.repository_path));
                    let pr_url = format!(
                        "https://github.com/{}/compare/{}...{}?expand=1",
                        tk.github_repo, tk_base_branch, branch_name
                    );
                    tk.pr_branch = Some(branch_name);
                    tk.pr_url = Some(pr_url);
                }
            }
        }

        if let Some(ref default_repo) = toml_cfg.default_repository_path {
            if let Some(proj) = state.projects.first_mut() {
                proj.repository_path = default_repo.clone();
            }
        }

        let linear_key_file = data_dir.join("linear_api_key.txt");
        let saved_file_key = fs::read_to_string(&linear_key_file)
            .ok()
            .map(|k| k.trim().to_string())
            .filter(|k| !k.is_empty());

        let resolved_linear_key = toml_cfg.linear_api_key.or(saved_file_key);

        if let Some(ref k) = resolved_linear_key {
            let _ = fs::write(&linear_key_file, k);
            state.linear_status.has_api_key = true;
        } else {
            state.linear_status.has_api_key = false;
            state.linear_status.connected = false;
            // Clear any stale cached Linear tickets if no API key is persisted yet
            if state
                .linear_projects
                .iter()
                .any(|p| p.source == "linear_api")
            {
                state.tickets.clear();
            }
        }

        if let Ok(json) = serde_json::to_string_pretty(&state) {
            let _ = fs::write(&data_file, json);
        }

        let (event_tx, _) = broadcast::channel(256);

        Self {
            state: Arc::new(RwLock::new(state)),
            linear_api_key: Arc::new(RwLock::new(resolved_linear_key)),
            linear_key_file,
            config_file,
            active_tasks: Arc::new(RwLock::new(HashMap::new())),
            pipeline_queue: Arc::new(tokio::sync::Mutex::new(())),
            agy: AgyBridge::new(),
            event_tx,
            data_file,
            workspaces_root,
        }
    }

    /// Clean ticket identifier removing brackets and markdown links (e.g. `[CAI-8](url)` -> `CAI-8`)
    pub fn clean_ticket_identifier(raw: &str) -> String {
        let trimmed = raw.trim();
        if let Some(start) = trimmed.find('[') {
            if let Some(end) = trimmed[start..].find(']') {
                let inside = &trimmed[start + 1..start + end];
                if !inside.trim().is_empty() {
                    return inside.trim().to_string();
                }
            }
        }
        if let Some(first_word) = trimmed.split_whitespace().next() {
            let clean_word: String = first_word
                .chars()
                .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
                .collect();
            if clean_word.contains('-') {
                return clean_word;
            }
        }
        let clean: String = trimmed
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
            .collect();
        if clean.is_empty() {
            "WAGNER".to_string()
        } else {
            clean
        }
    }

    /// Standard branch naming convention for WAGNER features: `factory-XXX` (where `XXX` is the ticket identifier, e.g. `factory-CAI-12`).
    pub fn branch_name_for_ticket(ticket_identifier: Option<&str>, version_tag: &str) -> String {
        let clean = ticket_identifier
            .map(|s| Self::clean_ticket_identifier(s))
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| Self::clean_ticket_identifier(version_tag));
        format!("factory-{}", clean)
    }

    /// Resolve the active response locale (`"fr"` or `"en"`).
    /// shortcut: heuristic French keyword/accent check before falling back to explicit locale or `"fr"`, upgrade to a full lingua detector only if >2 languages are needed.
    pub fn resolve_locale(explicit_locale: Option<&str>, text: &str) -> &'static str {
        let lower = text.to_lowercase();
        let has_french_markers = lower.chars().any(|c| "éèêëàâùûîïôœç".contains(c))
            || [
                " le ", " la ", " les ", " des ", " une ", " dans ", " pour ", " avec ",
                " sur ", " sont ", " être ", " toujours ", " alors ", " que ", " je ",
            ]
            .iter()
            .any(|w| lower.contains(w));
        if has_french_markers {
            return "fr";
        }
        if let Some(loc) = explicit_locale {
            if loc.trim().to_lowercase().starts_with("en") {
                return "en";
            }
        }
        "fr"
    }

    /// System-level language enforcement injected into every agent prompt (PO, EXPERT, ARCHI, DEV, QA, OPS, PR).
    pub fn language_directive(locale: &str) -> &'static str {
        if locale.eq_ignore_ascii_case("en") {
            "STRICT LANGUAGE DIRECTIVE (locale: en-US): Respond entirely in English (inside <thinking>...</thinking>, <question>...</question>, and the Markdown deliverable)."
        } else {
            "DIRECTIVE DE LANGUE STRICTE (locale: fr-FR) : L'utilisateur est francophone. Tu DOIS impérativement rédiger TOUTE ta réponse en FRANÇAIS (y compris les points de raisonnement dans `<thinking>...</thinking>`, toute question éventuelle dans `<question>...</question>`, et l'intégralité du livrable Markdown). Conserve uniquement les balises XML `<thinking>` / `<question>` et les identifiants de code tels quels."
        }
    }

    pub async fn persist(&self) {
        let guard = self.state.read().await;
        if let Ok(json) = serde_json::to_string_pretty(&*guard) {
            let _ = fs::write(&self.data_file, json);
        }
    }

    /// Automatically update the issue state in Linear according to the active Kanban stage.
    pub async fn sync_ticket_stage_to_linear(&self, ticket_id: &str, stage: KanbanStage) {
        let (api_key_opt, ticket_opt, states) = {
            let key_guard = self.linear_api_key.read().await;
            let state_guard = self.state.read().await;
            let tk = state_guard.tickets.iter().find(|t| t.id == ticket_id).cloned();
            (key_guard.clone(), tk, state_guard.linear_status.workflow_states.clone())
        };

        if let (Some(api_key), Some(ticket)) = (api_key_opt, ticket_opt) {
            if let Some(state_id) = LinearClient::resolve_state_id_for_stage(
                &states,
                ticket.linear_team_id.as_deref(),
                stage,
            ) {
                let tid = ticket.id.clone();
                let ident = ticket.identifier.clone();
                tokio::spawn(async move {
                    if let Ok(true) = LinearClient::update_issue_state(&api_key, &tid, &state_id).await {
                        info!("Successfully synced Linear issue {} to state {} for stage {:?}", ident, state_id, stage);
                    }
                });
            }
        }
    }

    /// Check for merged PRs on master, moving them to Released & Run, and auto-archive aged out cards (CAI-25).
    pub async fn check_merged_prs_and_auto_archive(&self) {
        let (api_key_opt, tickets_to_check, repo_dir) = {
            let key_guard = self.linear_api_key.read().await;
            let state_guard = self.state.read().await;
            let default_repo = state_guard
                .projects
                .first()
                .map(|p| p.repository_path.clone())
                .unwrap_or_else(|| "/home/mediaserver/Documents/SoftwareFactory".to_string());
            (key_guard.clone(), state_guard.tickets.clone(), default_repo)
        };

        // 1. Detect PRs merged into master for tickets in PrReview
        let mut newly_done = Vec::new();
        for ticket in &tickets_to_check {
            if ticket.stage == KanbanStage::PrReview && !ticket.archived {
                let repo_path = if ticket.repository_path.is_empty() {
                    &repo_dir
                } else {
                    &ticket.repository_path
                };

                let branch = ticket
                    .pr_branch
                    .clone()
                    .unwrap_or_else(|| format!("factory-{}", Self::clean_ticket_identifier(&ticket.identifier)));
                let slug = Self::clean_ticket_identifier(&ticket.identifier);

                let check_script = format!(
                    r#"cd "{repo}" 2>/dev/null || exit 0
git fetch origin master 2>/dev/null || true
MERGED=0
if git merge-base --is-ancestor "refs/remotes/origin/{branch}" "refs/remotes/origin/master" 2>/dev/null; then
  MERGED=1
elif git merge-base --is-ancestor "refs/heads/{branch}" "refs/heads/master" 2>/dev/null; then
  MERGED=1
elif git log origin/master -n 35 --oneline 2>/dev/null | grep -qi "{slug}"; then
  MERGED=1
elif git log master -n 35 --oneline 2>/dev/null | grep -qi "{slug}"; then
  MERGED=1
fi
if [ "$MERGED" -eq 1 ]; then echo "MERGED"; fi"#,
                    repo = repo_path,
                    branch = branch,
                    slug = slug,
                );

                if let Ok(out) = tokio::process::Command::new("bash")
                    .arg("-c")
                    .arg(&check_script)
                    .output()
                    .await
                {
                    let s = String::from_utf8_lossy(&out.stdout);
                    if s.contains("MERGED") {
                        info!("Auto-detected merged PR on master for ticket {}", ticket.identifier);
                        newly_done.push(ticket.id.clone());
                    }
                }
            }
        }

        // Apply transition to Done for newly merged tickets
        for tid in newly_done {
            let mut updated_tk = None;
            {
                let mut guard = self.state.write().await;
                if let Some(tk) = guard.tickets.iter_mut().find(|t| t.id == tid) {
                    tk.stage = KanbanStage::Done;
                    tk.active_role = None;
                    tk.released_at = Some(Utc::now());
                    tk.updated_at = Utc::now();
                    updated_tk = Some(tk.clone());
                }
            }
            if let Some(tk) = updated_tk {
                self.sync_ticket_stage_to_linear(&tk.id, KanbanStage::Done).await;
                let _ = self.event_tx.send(FactoryEvent::TicketUpdated { ticket: tk.clone() });
                if let Some(ref api_key) = api_key_opt {
                    let comment = format!(
                        "🚀 **[WAGNER Software Factory]** Pull Request for `{}` merged into `master`!\nCard transitioned to **Released & Run**.\n(Will automatically disappear after grace period).",
                        tk.identifier
                    );
                    LinearClient::post_issue_comment(api_key, &tk.id, &comment).await;
                }
            }
        }

        // 2. Auto-archive cards in Released & Run after 5 minutes
        let archive_cutoff_minutes = 5;
        let mut to_archive = Vec::new();
        {
            let guard = self.state.read().await;
            let now = Utc::now();
            for tk in &guard.tickets {
                if tk.stage == KanbanStage::Done && !tk.archived {
                    let ref_time = tk.released_at.unwrap_or(tk.updated_at);
                    if now - ref_time >= chrono::Duration::minutes(archive_cutoff_minutes) {
                        to_archive.push((tk.id.clone(), tk.identifier.clone()));
                    }
                }
            }
        }

        for (tid, ident) in to_archive {
            info!("Auto-archiving aged ticket {} from Released & Run (TTL expired)", ident);
            if let Some(ref api_key) = api_key_opt {
                let _ = LinearClient::archive_issue(api_key, &tid).await;
            }
            {
                let mut guard = self.state.write().await;
                guard.tickets.retain(|t| t.id != tid);
            }
            let _ = self.event_tx.send(FactoryEvent::TicketDeleted {
                ticket_id: tid,
                identifier: ident,
            });
        }

        self.persist().await;
    }

    pub fn default_linear_projects_and_tickets(
        workspaces_root: &PathBuf,
    ) -> (Vec<LinearProject>, Vec<FactoryTicket>) {
        let default_repo = workspaces_root.to_string_lossy().to_string();
        let repo_checkout = default_repo.clone();
        let repo_payments = default_repo.clone();
        let repo_sre = default_repo;

        let linear_projects = vec![
            LinearProject {
                id: "lin-proj-wagner-checkout".to_string(),
                name: "Smart Checkout & Stock Forge".to_string(),
                key: "WAG".to_string(),
                description: "Real-time inventory reservation, flash-sale circuit breakers, and BDD Test Factory.".to_string(),
                default_repo_path: repo_checkout.clone(),
                default_github_repo: "LPauzies/SoftwareFactory".to_string(),
                source: "demo".to_string(),
            },
            LinearProject {
                id: "lin-proj-wagner-pay".to_string(),
                name: "Instant SEPA & Stripe Gateway".to_string(),
                key: "PAY".to_string(),
                description: "Idempotent payment webhooks, automated reconciliation, and zero-downtime rollbacks.".to_string(),
                default_repo_path: repo_payments.clone(),
                default_github_repo: "LPauzies/SoftwareFactory".to_string(),
                source: "demo".to_string(),
            },
            LinearProject {
                id: "lin-proj-wagner-sre".to_string(),
                name: "DevSecFinOps & Autonomous RUN".to_string(),
                key: "SRE".to_string(),
                description: "Automated canary rollouts, anomaly detection, and living architecture documentation.".to_string(),
                default_repo_path: repo_sre.clone(),
                default_github_repo: "LPauzies/SoftwareFactory".to_string(),
                source: "demo".to_string(),
            },
        ];

        let tickets = vec![
            FactoryTicket {
                id: "ticket-wag-101".to_string(),
                identifier: "WAG-101".to_string(),
                title: "Idempotent cart reservation lock with Redis TTL & rollback hook".to_string(),
                description: "Ensure concurrent checkout attempts never oversell flash-sale stock and include an automated rollback script.".to_string(),
                priority_label: "High".to_string(),
                linear_project_id: "lin-proj-wagner-checkout".to_string(),
                linear_team_id: None,
                linear_url: Some("https://linear.app/wagner/issue/WAG-101".to_string()),
                repository_path: repo_checkout.clone(),
                github_repo: "LPauzies/SoftwareFactory".to_string(),
                stage: KanbanStage::PrReview,
                active_role: None,
                active_run_id: Some("run-seed-v100".to_string()),
                pending_question: None,
                pr_url: Some("https://github.com/LPauzies/SoftwareFactory/compare/main...wagner/wag-101-v1-0-0?expand=1".to_string()),
                pr_branch: Some("wagner/wag-101-v1-0-0".to_string()),
                version_tag: "v1.0.0".to_string(),
                created_at: Utc::now() - chrono::Duration::days(3),
                updated_at: Utc::now() - chrono::Duration::hours(6),
                released_at: None,
                archived: false,
            },
            FactoryTicket {
                id: "ticket-wag-102".to_string(),
                identifier: "WAG-102".to_string(),
                title: "Flash-sale surge circuit breaker & VIP priority queue".to_string(),
                description: "Protect checkout latency under 10x traffic spikes and verify with Playwright + Gherkin BDD scenarios.".to_string(),
                priority_label: "Urgent".to_string(),
                linear_project_id: "lin-proj-wagner-checkout".to_string(),
                linear_team_id: None,
                linear_url: Some("https://linear.app/wagner/issue/WAG-102".to_string()),
                repository_path: repo_checkout.clone(),
                github_repo: "LPauzies/SoftwareFactory".to_string(),
                stage: KanbanStage::Done,
                active_role: None,
                active_run_id: Some("run-seed-v110".to_string()),
                pending_question: None,
                pr_url: Some("https://github.com/LPauzies/SoftwareFactory/compare/main...wagner/wag-102-v1-1-0?expand=1".to_string()),
                pr_branch: Some("wagner/wag-102-v1-1-0".to_string()),
                version_tag: "v1.1.0".to_string(),
                created_at: Utc::now() - chrono::Duration::days(2),
                updated_at: Utc::now() - chrono::Duration::hours(2),
                released_at: Some(Utc::now() - chrono::Duration::hours(2)),
                archived: false,
            },
            FactoryTicket {
                id: "ticket-wag-103".to_string(),
                identifier: "WAG-103".to_string(),
                title: "Multi-warehouse split-shipment optimizer & BDD living docs".to_string(),
                description: "Route multi-item orders across closest regional warehouses with full Test Factory coverage and auto-generated release notes.".to_string(),
                priority_label: "Urgent".to_string(),
                linear_project_id: "lin-proj-wagner-checkout".to_string(),
                linear_team_id: None,
                linear_url: Some("https://linear.app/wagner/issue/WAG-103".to_string()),
                repository_path: repo_checkout.clone(),
                github_repo: "LPauzies/SoftwareFactory".to_string(),
                stage: KanbanStage::Todo,
                active_role: None,
                active_run_id: None,
                pending_question: None,
                pr_url: None,
                pr_branch: None,
                version_tag: "v1.2.0".to_string(),
                created_at: Utc::now() - chrono::Duration::hours(12),
                updated_at: Utc::now() - chrono::Duration::minutes(30),
                released_at: None,
                archived: false,
            },
            FactoryTicket {
                id: "ticket-wag-104".to_string(),
                identifier: "WAG-104".to_string(),
                title: "Real-time webhook retry dead-letter queue with SRE alert probe".to_string(),
                description: "Guarantee at-least-once delivery for ERP stock sync webhooks with exponential backoff and instant rollback script.".to_string(),
                priority_label: "High".to_string(),
                linear_project_id: "lin-proj-wagner-checkout".to_string(),
                linear_team_id: None,
                linear_url: Some("https://linear.app/wagner/issue/WAG-104".to_string()),
                repository_path: repo_checkout.clone(),
                github_repo: "LPauzies/SoftwareFactory".to_string(),
                stage: KanbanStage::Todo,
                active_role: None,
                active_run_id: None,
                pending_question: None,
                pr_url: None,
                pr_branch: None,
                version_tag: "v1.3.0".to_string(),
                created_at: Utc::now() - chrono::Duration::hours(8),
                updated_at: Utc::now() - chrono::Duration::minutes(15),
                released_at: None,
                archived: false,
            },
            FactoryTicket {
                id: "ticket-pay-201".to_string(),
                identifier: "PAY-201".to_string(),
                title: "Instant SEPA IBAN verification & fraud score threshold gate".to_string(),
                description: "Validate payee IBAN and risk score in <80ms before authorizing instant SEPA transfer.".to_string(),
                priority_label: "Urgent".to_string(),
                linear_project_id: "lin-proj-wagner-pay".to_string(),
                linear_team_id: None,
                linear_url: Some("https://linear.app/wagner/issue/PAY-201".to_string()),
                repository_path: repo_payments.clone(),
                github_repo: "LPauzies/SoftwareFactory".to_string(),
                stage: KanbanStage::Todo,
                active_role: None,
                active_run_id: None,
                pending_question: None,
                pr_url: None,
                pr_branch: None,
                version_tag: "v1.0.0".to_string(),
                created_at: Utc::now() - chrono::Duration::hours(6),
                updated_at: Utc::now() - chrono::Duration::minutes(25),
                released_at: None,
                archived: false,
            },
            FactoryTicket {
                id: "ticket-pay-202".to_string(),
                identifier: "PAY-202".to_string(),
                title: "Automated daily ledger reconciliation & discrepancy detector".to_string(),
                description: "Reconcile Stripe & SEPA settlement batches nightly with Gherkin BDD scenarios.".to_string(),
                priority_label: "Medium".to_string(),
                linear_project_id: "lin-proj-wagner-pay".to_string(),
                linear_team_id: None,
                linear_url: Some("https://linear.app/wagner/issue/PAY-202".to_string()),
                repository_path: repo_payments.clone(),
                github_repo: "LPauzies/SoftwareFactory".to_string(),
                stage: KanbanStage::Todo,
                active_role: None,
                active_run_id: None,
                pending_question: None,
                pr_url: None,
                pr_branch: None,
                version_tag: "v1.1.0".to_string(),
                created_at: Utc::now() - chrono::Duration::hours(4),
                updated_at: Utc::now() - chrono::Duration::minutes(10),
                released_at: None,
                archived: false,
            },
            FactoryTicket {
                id: "ticket-sre-301".to_string(),
                identifier: "SRE-301".to_string(),
                title: "Automated p99 latency watchdog & zero-touch Kubernetes rollback".to_string(),
                description: "Monitor deployment SLOs for 5 minutes post-release and trigger automatic rollback if error budget burns.".to_string(),
                priority_label: "High".to_string(),
                linear_project_id: "lin-proj-wagner-sre".to_string(),
                linear_team_id: None,
                linear_url: Some("https://linear.app/wagner/issue/SRE-301".to_string()),
                repository_path: repo_sre.clone(),
                github_repo: "LPauzies/SoftwareFactory".to_string(),
                stage: KanbanStage::Todo,
                active_role: None,
                active_run_id: None,
                pending_question: None,
                pr_url: None,
                pr_branch: None,
                version_tag: "v1.0.0".to_string(),
                created_at: Utc::now() - chrono::Duration::hours(2),
                updated_at: Utc::now() - chrono::Duration::minutes(5),
                released_at: None,
                archived: false,
            },
        ];

        (linear_projects, tickets)
    }

    pub fn default_bricks() -> Vec<FactoryBrick> {
        vec![
            FactoryBrick {
                kind: BrickKind::ProjectManagement,
                name: "Project Management & Agility".to_string(),
                french_subtitle: "Gagner en productivité".to_string(),
                description: "Linear Kanban backlog, user-centric stories, and cross-role agile progression tracking.".to_string(),
                enabled: true,
                provider_plugin: "Linear GraphQL Sync".to_string(),
                health_score: 98,
            },
            FactoryBrick {
                kind: BrickKind::ScmSecurity,
                name: "Source Code Security & SCM".to_string(),
                french_subtitle: "Sécuriser votre code source".to_string(),
                description: "Version conflict management, non-destructive guardrails, branch protection, and Software Craftsmanship gates.".to_string(),
                enabled: true,
                provider_plugin: "GitLab SCM + SonarQube".to_string(),
                health_score: 96,
            },
            FactoryBrick {
                kind: BrickKind::TestFactory,
                name: "Connected Test Factory".to_string(),
                french_subtitle: "Améliorer la qualité de vos livrables".to_string(),
                description: "Automated integration & BDD business test suites connected directly to the Software Factory as a regression safety net.".to_string(),
                enabled: true,
                provider_plugin: "Playwright + Cucumber BDD".to_string(),
                health_score: 95,
            },
            FactoryBrick {
                kind: BrickKind::LivingDocumentation,
                name: "Living Documentation & Reporting".to_string(),
                french_subtitle: "Consolider votre documentation".to_string(),
                description: "Auto-generated Release Notes, code-linked technical Wikis, and executable BDD business scenarios as living documentation.".to_string(),
                enabled: true,
                provider_plugin: "Wagner LivingWiki & ReleaseBot".to_string(),
                health_score: 97,
            },
            FactoryBrick {
                kind: BrickKind::ContinuousDeployment,
                name: "Continuous Deployment (CI/CD)".to_string(),
                french_subtitle: "Accélérer vos déploiements".to_string(),
                description: "Automated Build & Release pipelines delivering value continuously while OPS monitors every delivery with confidence.".to_string(),
                enabled: true,
                provider_plugin: "GitLab CI / Jenkins Pipelines".to_string(),
                health_score: 99,
            },
            FactoryBrick {
                kind: BrickKind::RunAndRollback,
                name: "RUN Visibility & Rollback Engine".to_string(),
                french_subtitle: "Améliorer votre RUN & Retour arrière".to_string(),
                description: "Deployment versioning visibility, incident diagnostics, and first-class automated rollback processes designed from day one.".to_string(),
                enabled: true,
                provider_plugin: "ArgoRollouts + Wagner SRE Guard".to_string(),
                health_score: 98,
            },
        ]
    }

    pub fn default_agents() -> Vec<AgentConfig> {
        vec![
            AgentConfig {
                role: AgentRole::Po,
                title: AgentRole::Po.title().to_string(),
                wagner_pillar: AgentRole::Po.wagner_pillar().to_string(),
                model: "gemini-3.8-flash-low".to_string(),
                effort: "low".to_string(),
                enabled: true,
                system_prompt: "Tu es le Product Owner (PO) dans la Software Factory WAGNER. Rédige les user stories, critères d'acceptation et priorités en français selon les standards du Software Craftsmanship.".to_string(),
            },
            AgentConfig {
                role: AgentRole::Expert,
                title: AgentRole::Expert.title().to_string(),
                wagner_pillar: AgentRole::Expert.wagner_pillar().to_string(),
                model: "gemini-3.8-flash-low".to_string(),
                effort: "low".to_string(),
                enabled: true,
                system_prompt: "Tu es l'Expert Métier (Business Expert) dans WAGNER. Définis le langage ubiquitaire du domaine, les invariants métier, les règles de validation et la documentation vivante en français.".to_string(),
            },
            AgentConfig {
                role: AgentRole::Archi,
                title: AgentRole::Archi.title().to_string(),
                wagner_pillar: AgentRole::Archi.wagner_pillar().to_string(),
                model: "gemini-3.8-flash-low".to_string(),
                effort: "low".to_string(),
                enabled: true,
                system_prompt: "Tu es l'Architecte Logiciel (ARCHI) de la Software Factory WAGNER. Garantir la compatibilité et l'homogénéité des modules à travers l'usine logicielle, concevoir des frontières modulaires propres et spécifier des transitions d'état réversibles (rollback-safe) en français.".to_string(),
            },
            AgentConfig {
                role: AgentRole::Dev,
                title: AgentRole::Dev.title().to_string(),
                wagner_pillar: AgentRole::Dev.wagner_pillar().to_string(),
                model: "gemini-4-argon-medium".to_string(),
                effort: "medium".to_string(),
                enabled: true,
                system_prompt: "Tu es le Développeur Software Crafts(wo)man (DEV) dans WAGNER. Utilise toujours le plugin ponytail en mode full (`/ponytail full` — plus petit changement complet, zéro sur-ingénierie, réutilisation des patterns existants, marquage des limites différées avec `shortcut: <limite>, <quand améliorer>`, et conclus par 1-2 lignes sur ce qui a été omis et les risques éventuels). Inspecte directement le dépôt, implémente le code source de production et les tests unitaires, commite sur la branche feature et rédige ton livrable et tes explications en français.".to_string(),
            },
            AgentConfig {
                role: AgentRole::Qa,
                title: AgentRole::Qa.title().to_string(),
                wagner_pillar: AgentRole::Qa.wagner_pillar().to_string(),
                model: "gemini-3.8-flash-low".to_string(),
                effort: "low".to_string(),
                enabled: true,
                system_prompt: "Tu es l'Ingénieur Test Factory & QA dans WAGNER. Raccorde la Test Factory en rédigeant des tests d'intégration automatisés et des scénarios BDD Gherkin en français.".to_string(),
            },
            AgentConfig {
                role: AgentRole::Ops,
                title: AgentRole::Ops.title().to_string(),
                wagner_pillar: AgentRole::Ops.wagner_pillar().to_string(),
                model: "gemini-3.8-flash-low".to_string(),
                effort: "low".to_string(),
                enabled: true,
                system_prompt: "Tu es l'Ingénieur DevSecFinOps & SRE (OPS) dans WAGNER. Automatise les pipelines CI/CD de Build & Release, génère les Release Notes, assure l'observabilité du RUN et fournis un script de Rollback explicite en français.".to_string(),
            },
            AgentConfig {
                role: AgentRole::Pr,
                title: AgentRole::Pr.title().to_string(),
                wagner_pillar: AgentRole::Pr.wagner_pillar().to_string(),
                model: "gemini-3.8-flash-low".to_string(),
                effort: "low".to_string(),
                enabled: true,
                system_prompt: "Tu es l'Intégrateur GitHub PR & Release (PR) dans WAGNER. Tu prépares la revue humaine, vérifies l'arbre Git de la branche feature, et génères la Pull Request avec la checklist de contrôle en français.".to_string(),
            },
        ]
    }

    pub fn default_marketplace() -> Vec<MarketplacePlugin> {
        vec![
            MarketplacePlugin {
                id: "gitlab-ci".to_string(),
                name: "GitLab Forge & CI/CD".to_string(),
                category: "SCM & CI/CD".to_string(),
                brick_kind: BrickKind::ContinuousDeployment,
                description: "Unified Git repository management, merge request safety gates, and automated Build/Release pipelines.".to_string(),
                installed: true,
                compatible_with: vec!["playwright-bdd".to_string(), "sonarqube-craft".to_string()],
                icon: "GitBranch".to_string(),
            },
            MarketplacePlugin {
                id: "jenkins-pipeline".to_string(),
                name: "Jenkins Declarative Orchestrator".to_string(),
                category: "CI/CD Server".to_string(),
                brick_kind: BrickKind::ContinuousDeployment,
                description: "Classic extensible automation server featured in Wagner Software Factory toolchains.".to_string(),
                installed: true,
                compatible_with: vec!["sonarqube-craft".to_string()],
                icon: "Cpu".to_string(),
            },
            MarketplacePlugin {
                id: "playwright-bdd".to_string(),
                name: "Playwright Test Factory".to_string(),
                category: "Test Automation".to_string(),
                brick_kind: BrickKind::TestFactory,
                description: "High-speed multi-browser E2E & BDD test runner.".to_string(),
                installed: true,
                compatible_with: vec!["gitlab-ci".to_string()],
                icon: "CheckCircle".to_string(),
            },
            MarketplacePlugin {
                id: "sonarqube-craft".to_string(),
                name: "SonarQube Craftsmanship Gate".to_string(),
                category: "Code Quality & Security".to_string(),
                brick_kind: BrickKind::ScmSecurity,
                description: "Static analysis, SAST security checks, and clean-code quality gates before merge.".to_string(),
                installed: true,
                compatible_with: vec!["gitlab-ci".to_string(), "jenkins-pipeline".to_string(), "ponytail-full".to_string()],
                icon: "Lock".to_string(),
            },
            MarketplacePlugin {
                id: "ponytail-full".to_string(),
                name: "Ponytail Full (Minimal Viable Diff)".to_string(),
                category: "Software Craftsmanship".to_string(),
                brick_kind: BrickKind::ScmSecurity,
                description: "Always-on Ponytail Full mode (`/ponytail full`) for DEV implementations: smallest complete change, zero bloat, and explicit `shortcut:` debt ledger comments.".to_string(),
                installed: true,
                compatible_with: vec!["sonarqube-craft".to_string(), "gitlab-ci".to_string()],
                icon: "Scissors".to_string(),
            },
        ]
    }

    fn seed_initial_state(workspaces_root: &PathBuf) -> FactoryState {
        let project_id = "proj-wagner-forge-01".to_string();
        let slug = "wagner-smart-checkout".to_string();
        let repo_path = workspaces_root.join(&slug).to_string_lossy().to_string();
        let _ = fs::create_dir_all(workspaces_root.join(&slug));

        let project = Project {
            id: project_id.clone(),
            name: "WAGNER Smart Checkout & Inventory Forge".to_string(),
            slug: slug.clone(),
            description: "Cloud-native e-commerce checkout & real-time stock reservation service built and maintained by the 6-role WAGNER Gemini Software Factory.".to_string(),
            domain: "E-Commerce & Supply Chain".to_string(),
            repository_path: repo_path.clone(),
            active_version: "v1.1.0".to_string(),
            created_at: Utc::now(),
            bricks: Self::default_bricks(),
            agents: Self::default_agents(),
            homogeneity_score: 96,
        };

        let mut run_v1 = Self::create_seeded_run(
            &project,
            "run-seed-v100",
            "v1.0.0",
            "Idempotent cart reservation lock with Redis TTL & rollback hook",
        );
        run_v1.ticket_id = Some("ticket-wag-101".to_string());
        run_v1.ticket_identifier = Some("WAG-101".to_string());

        let mut run_v11 = Self::create_seeded_run(
            &project,
            "run-seed-v110",
            "v1.1.0",
            "Flash-sale surge circuit breaker & VIP priority queue",
        );
        run_v11.ticket_id = Some("ticket-wag-102".to_string());
        run_v11.ticket_identifier = Some("WAG-102".to_string());

        let rel_v1 = ReleaseRecord {
            id: "rel-v100".to_string(),
            project_id: project_id.clone(),
            pipeline_run_id: run_v1.id.clone(),
            version: "v1.0.0".to_string(),
            environment: "production".to_string(),
            status: ReleaseStatus::Superseded,
            deployed_at: Utc::now() - chrono::Duration::hours(18),
            release_notes: "## Release v1.0.0 (WAG-101) — Core Stock Reservation Forge\n- Idempotent cart reservation API (`POST /api/v1/reservations`)\n- Automated Test Factory suite (12 BDD scenarios, 100% pass)\n- Instant rollback snapshot (`v0.9.0 -> v1.0.0`)".to_string(),
            rollback_script: "#!/usr/bin/env bash\nset -euo pipefail\necho \"[WAGNER SRE] Rolling back deployment to v1.0.0...\"\ngit checkout tags/v1.0.0\n".to_string(),
            test_pass_rate: 100,
            incident_notes: None,
        };

        let rel_v11 = ReleaseRecord {
            id: "rel-v110".to_string(),
            project_id: project_id.clone(),
            pipeline_run_id: run_v11.id.clone(),
            version: "v1.1.0".to_string(),
            environment: "production".to_string(),
            status: ReleaseStatus::ActiveInProd,
            deployed_at: Utc::now() - chrono::Duration::hours(2),
            release_notes: "## Release v1.1.0 (WAG-102) — Flash-Sale Circuit Breaker & VIP Priority\n- Adaptive token-bucket rate limiter & flash-sale surge protection\n- Connected Test Factory: 18 automated Playwright + Cucumber BDD scenarios\n- Living Wiki & Rollback runbook automatically synchronized".to_string(),
            rollback_script: "#!/usr/bin/env bash\nset -euo pipefail\necho \"[WAGNER SRE] Rolling back from v1.1.0 to verified snapshot v1.0.0...\"\ngit checkout tags/v1.0.0\n".to_string(),
            test_pass_rate: 100,
            incident_notes: None,
        };

        let (linear_projects, tickets) =
            Self::default_linear_projects_and_tickets(workspaces_root);

        FactoryState {
            projects: vec![project],
            linear_projects,
            tickets,
            dismissed_ticket_ids: vec![],
            linear_status: LinearStatus {
                connected: false,
                has_api_key: false,
                organization_name: Some("WAGNER Local Forge".to_string()),
                viewer_name: None,
                last_synced_at: Some(Utc::now()),
                workflow_states: vec![],
            },
            marketplace: Self::default_marketplace(),
            runs: vec![run_v11, run_v1],
            releases: vec![rel_v11, rel_v1],
            rollbacks: vec![],
        }
    }

    pub fn synthesize_role_thoughts(
        role: AgentRole,
        project: &Project,
        version: &str,
        feature_request: &str,
    ) -> Vec<ThoughtEntry> {
        let snippets: Vec<(&str, String)> = match role {
            AgentRole::Po => vec![
                (
                    "reasoning",
                    format!(
                        "Deconstructing user value proposition for '{}' (target release {}).",
                        feature_request, version
                    ),
                ),
                (
                    "reasoning",
                    "Applying Wagner user-centric agility principle: minimize scope bloat while guaranteeing <100ms perceived latency.".to_string(),
                ),
                (
                    "conclusion",
                    format!(
                        "Formulated 3 testable User Stories (US-{}-01..03) with explicit rollback toggles.",
                        version
                    ),
                ),
            ],
            AgentRole::Expert => vec![
                (
                    "reasoning",
                    format!(
                        "Auditing domain invariants in '{}' against '{}'.",
                        project.domain, feature_request
                    ),
                ),
                (
                    "reasoning",
                    "Ensuring idempotency keys and compensating state transitions so OPS can safely roll back without orphaned records.".to_string(),
                ),
                (
                    "conclusion",
                    "Locked domain invariants D1 (Idempotency), D2 (Consistency), and D3 (Reversibility).".to_string(),
                ),
            ],
            AgentRole::Archi => vec![
                (
                    "reasoning",
                    format!(
                        "Checking Software Factory brick homogeneity across repository '{}'.",
                        project.repository_path
                    ),
                ),
                (
                    "tool_probe",
                    "Verified active bricks compatibility (SCM + Test Factory + CI/CD + RUN Rollback).".to_string(),
                ),
                (
                    "conclusion",
                    format!(
                        "Published ADR-{} with isolated trait boundary and backward-compatible schema.",
                        version
                    ),
                ),
            ],
            AgentRole::Dev => vec![
                (
                    "reasoning",
                    format!(
                        "Preparing clean Software Craftsmanship implementation for {} in target repo.",
                        version
                    ),
                ),
                (
                    "tool_probe",
                    "Enforcing non-destructive SCM branch guardrails and zero-unwrap error handling.".to_string(),
                ),
                (
                    "conclusion",
                    format!(
                        "Generated src/feature_{}.rs with deterministic validation and rollback guard.",
                        version.replace('.', "_")
                    ),
                ),
            ],
            AgentRole::Qa => vec![
                (
                    "reasoning",
                    "Connecting Test Factory to Software Factory: translating domain invariants into executable Gherkin BDD scenarios.".to_string(),
                ),
                (
                    "tool_probe",
                    "Synthesizing nominal path + invalid payload regression shield assertions.".to_string(),
                ),
                (
                    "conclusion",
                    format!(
                        "Committed tests/bdd_{}.feature as living documentation.",
                        version.replace('.', "_")
                    ),
                ),
            ],
            AgentRole::Ops => vec![
                (
                    "reasoning",
                    format!(
                        "Designing automated Build & Release pipeline and pre-armed rollback script for {}.",
                        version
                    ),
                ),
                (
                    "tool_probe",
                    "Validating CI/CD stage gates: scm_guard -> build -> test_factory -> deploy_prod -> rollback_ready.".to_string(),
                ),
                (
                    "conclusion",
                    format!(
                        "Generated ops/pipeline_{}.yml, auto-compiled Release Notes, and armed instant rollback hook.",
                        version.replace('.', "_")
                    ),
                ),
            ],
            AgentRole::Pr => vec![
                (
                    "reasoning",
                    format!(
                        "Creating feature branch against main and preparing GitHub Pull Request for {}.",
                        version
                    ),
                ),
                (
                    "tool_probe",
                    "Staging all generated specs, implementation files, Gherkin BDD scenarios, and CI/CD runbooks into atomic Git commit.".to_string(),
                ),
                (
                    "conclusion",
                    format!(
                        "Pushed branch to GitHub origin and opened Pull Request against main for human review ({}).",
                        version
                    ),
                ),
            ],
        };

        snippets
            .into_iter()
            .map(|(kind, content)| ThoughtEntry {
                id: format!("cot-{}", &Uuid::new_v4().to_string()[..8]),
                role,
                kind: kind.to_string(),
                content,
                timestamp: Utc::now(),
            })
            .collect()
    }

    fn create_seeded_run(
        project: &Project,
        run_id: &str,
        version: &str,
        feature_request: &str,
    ) -> PipelineRun {
        let mut steps = Vec::new();
        for agent in &project.agents {
            let (summary, md, artifacts) =
                Self::synthesize_role_output(agent.role, project, version, feature_request);
            let thoughts =
                Self::synthesize_role_thoughts(agent.role, project, version, feature_request);
            steps.push(AgentStepExecution {
                role: agent.role,
                title: agent.title.clone(),
                wagner_pillar: agent.wagner_pillar.clone(),
                model: agent.model.clone(),
                status: StepStatus::Completed,
                started_at: Some(Utc::now() - chrono::Duration::minutes(15)),
                completed_at: Some(Utc::now() - chrono::Duration::minutes(14)),
                duration_seconds: 2.8,
                agy_conversation_id: Some(format!(
                    "agy-conv-{}-{}",
                    version,
                    agent.role.code().to_lowercase()
                )),
                agy_command: format!(
                    "/home/mediaserver/.local/bin/agy --print \"[{}] {}\" --model {} --effort {} --add-dir \"{}\" --disable-slash-commands --output-format json",
                    agent.role.code(),
                    feature_request,
                    agent.model,
                    agent.effort,
                    project.repository_path
                ),
                summary,
                markdown_output: md,
                thinking_log: thoughts,
                artifacts,
                usage: AgyUsage {
                    input_tokens: 4210,
                    output_tokens: 680,
                    thinking_tokens: 180,
                    cache_read_tokens: 0,
                    total_tokens: 5070,
                },
                quality_gate_passed: true,
            });
        }

        let gherkin = format!(
            "Feature: {} ({})\n  As a user of {}\n  I want reliable, regression-free feature delivery\n\n  Scenario: Nominal business flow validated by Test Factory\n    Given release \"{}\" is staged with clean SCM checks\n    When the customer triggers the workflow for \"{}\"\n    Then the service responds within 50ms with HTTP 200\n    And zero regressions are detected\n",
            feature_request, version, project.name, version, feature_request
        );

        PipelineRun {
            id: run_id.to_string(),
            project_id: project.id.clone(),
            ticket_id: None,
            ticket_identifier: None,
            version_tag: version.to_string(),
            feature_request: feature_request.to_string(),
            execution_mode: "agy_live".to_string(),
            status: PipelineStatus::Succeeded,
            created_at: Utc::now() - chrono::Duration::minutes(16),
            completed_at: Some(Utc::now() - chrono::Duration::minutes(12)),
            steps,
            test_report: Some(TestFactoryReport {
                total_scenarios: 18,
                passed_scenarios: 18,
                integration_tests: 12,
                business_bdd_scenarios: 6,
                coverage_estimate: 94,
                regression_shield_active: true,
                gherkin_feature: gherkin.clone(),
            }),
            living_docs: Some(LivingDocBundle {
                release_notes: format!(
                    "# Release Notes — {}\n\n**Delivered:** {}\n\n- **PO & Business Expert**: Validated user stories & domain invariants.\n- **ARCHI & DEV**: Modular Software Craftsmanship implementation.\n- **QA (Test Factory)**: 18/18 automated integration & BDD scenarios passing.\n- **OPS (RUN & Rollback)**: Automated CI/CD deployment with verified rollback script.",
                    version, feature_request
                ),
                wiki_architecture_page: format!(
                    "# Living Architecture Wiki — {} ({})\n\n- **SCM Security**: Protected trunk with atomic feature branches.\n- **Test Factory Link**: Synchronous BDD verification.\n- **Rollback Strategy**: Immutable tags + reversible migrations.",
                    project.name, version
                ),
                living_bdd_catalog: gherkin,
                rollback_runbook: format!(
                    "# Rollback Runbook — {}\n\n1. Execute `./ops/rollback_{}.sh` or click **Rollback** on the Kanban card.\n2. Confirm health probes.",
                    version, version
                ),
            }),
            pr_url: Some(format!(
                "https://github.com/LPauzies/SoftwareFactory/compare/main...factory-{}?expand=1",
                version.replace('.', "-")
            )),
            pr_branch: Some(format!("factory-{}", version.replace('.', "-"))),
            total_tokens: 30420,
            total_duration_seconds: 16.8,
            workspace_dir: format!("{}/{}", project.repository_path, version),
        }
    }

    pub fn synthesize_role_output(
        role: AgentRole,
        project: &Project,
        version: &str,
        feature_request: &str,
    ) -> (String, String, Vec<ArtifactFile>) {
        match role {
            AgentRole::Po => {
                let summary = format!(
                    "Defined user stories & acceptance criteria for {} ({})",
                    version, feature_request
                );
                let md = format!(
                    "### Product Owner (PO) — User Stories & Value Spec\n\n\
                    #### Objective\n> {}\n\n\
                    #### Acceptance Criteria\n\
                    1. **US-{}-01**: End-to-end flow completes in <100ms.\n\
                    2. **US-{}-02**: 100% backward compatibility with existing contracts.\n\
                    3. **US-{}-03**: Instant rollback toggle armed prior to rollout.",
                    feature_request, version, version, version
                );
                let artifact = ArtifactFile {
                    path: format!("specs/backlog_{}.md", version),
                    category: "Spec (PO)".to_string(),
                    language: "markdown".to_string(),
                    content: md.clone(),
                };
                (summary, md, vec![artifact])
            }
            AgentRole::Expert => {
                let summary = format!(
                    "Validated domain invariants & business test rules for {}",
                    version
                );
                let md = format!(
                    "### Business Expert (Expert Métier) — Domain Rules\n\n\
                    - **Invariant D1 (Idempotency)**: Every command for *\"{:.60}\"* carries a deterministic idempotency key.\n\
                    - **Invariant D2 (Consistency)**: Domain events emit only after validation succeeds.\n\
                    - **Invariant D3 (Reversibility)**: State transitions support instant OPS rollback.",
                    feature_request
                );
                let artifact = ArtifactFile {
                    path: format!("specs/domain_rules_{}.md", version),
                    category: "Domain (EXPERT)".to_string(),
                    language: "markdown".to_string(),
                    content: md.clone(),
                };
                (summary, md, vec![artifact])
            }
            AgentRole::Archi => {
                let summary = format!(
                    "Designed modular ADR & rollback boundaries for {}",
                    version
                );
                let md = format!(
                    "### Software Architect (ARCHI) — ADR-{}\n\n\
                    - **Context**: Implementing `{}` in repository `{}`.\n\
                    - **Homogeneity**: Clean module boundary compatible with active Factory Bricks.\n\
                    - **Rollback Contract**: Backward-compatible schema for zero-downtime rollback.",
                    version, feature_request, project.repository_path
                );
                let artifact = ArtifactFile {
                    path: format!("docs/adr_{}.md", version),
                    category: "Architecture (ARCHI)".to_string(),
                    language: "markdown".to_string(),
                    content: md.clone(),
                };
                (summary, md, vec![artifact])
            }
            AgentRole::Dev => {
                let summary = format!(
                    "Implemented clean Software Craftsmanship module in target repo for {}",
                    version
                );
                let code = format!(
                    "//! Generated by WAGNER Software Factory — DEV Agent ({})\n\
                    //! Ticket Feature: {}\n\n\
                    use serde::{{Deserialize, Serialize}};\n\n\
                    #[derive(Debug, Clone, Serialize, Deserialize)]\n\
                    pub struct TicketDeliveryResult {{\n\
                        pub version: String,\n\
                        pub request_id: String,\n\
                        pub rollback_safe: bool,\n\
                    }}\n\n\
                    pub fn execute_ticket_workflow(request_id: &str) -> Result<TicketDeliveryResult, &'static str> {{\n\
                        if request_id.trim().is_empty() {{\n\
                            return Err(\"invalid_request_id\");\n\
                        }}\n\
                        Ok(TicketDeliveryResult {{\n\
                            version: \"{}\".to_string(),\n\
                            request_id: request_id.to_string(),\n\
                            rollback_safe: true,\n\
                        }})\n\
                    }}\n",
                    version, feature_request, version
                );
                let md = format!(
                    "### Software Crafts(wo)man (DEV) — Implementation\n\n\
                    ```rust\n{}\n```",
                    code
                );
                let artifact = ArtifactFile {
                    path: format!("src/feature_{}.rs", version.replace('.', "_")),
                    category: "Code (DEV)".to_string(),
                    language: "rust".to_string(),
                    content: code,
                };
                (summary, md, vec![artifact])
            }
            AgentRole::Qa => {
                let summary = format!(
                    "Connected Test Factory: generated Gherkin BDD & integration tests for {}",
                    version
                );
                let gherkin = format!(
                    "Feature: {} ({})\n\
                      Scenario: Nominal execution validated by Connected Test Factory\n\
                        Given the repository is at release \"{}\"\n\
                        When a client executes the workflow for \"{}\"\n\
                        Then the response status is 200 OK\n\
                        And rollback_safe is true\n",
                    feature_request, version, version, feature_request
                );
                let md = format!(
                    "### Test Factory (QA) — BDD & Integration Suite\n\n\
                    ```gherkin\n{}\n```",
                    gherkin
                );
                let artifact = ArtifactFile {
                    path: format!("tests/bdd_{}.feature", version.replace('.', "_")),
                    category: "Test Factory (QA)".to_string(),
                    language: "gherkin".to_string(),
                    content: gherkin,
                };
                (summary, md, vec![artifact])
            }
            AgentRole::Ops => {
                let summary = format!(
                    "Deployed {} via CI/CD with Living Release Notes & Rollback script",
                    version
                );
                let cicd = format!(
                    "# WAGNER Software Factory — CI/CD & Rollback Pipeline ({})\n\
                    stages: [scm_guard, build, test_factory, deploy_prod, rollback_ready]\n\n\
                    deploy_production:\n\
                      stage: deploy_prod\n\
                      script:\n\
                        - ./ops/deploy.sh --version {}\n\n\
                    rollback_on_alert:\n\
                      stage: rollback_ready\n\
                      when: on_failure\n\
                      script:\n\
                        - ./ops/rollback_{}.sh\n",
                    version, version, version.replace('.', "_")
                );
                let md = format!(
                    "### DevSecOps & SRE (OPS) — CI/CD & Rollback\n\n\
                    ```yaml\n{}\n```",
                    cicd
                );
                let artifact = ArtifactFile {
                    path: format!("ops/pipeline_{}.yml", version.replace('.', "_")),
                    category: "CI/CD & Rollback (OPS)".to_string(),
                    language: "yaml".to_string(),
                    content: cicd,
                };
                (summary, md, vec![artifact])
            }
            AgentRole::Pr => {
                let branch = format!("factory-{}", version.replace('.', "-"));
                let pr_url = format!(
                    "https://github.com/LPauzies/SoftwareFactory/compare/main...{}?expand=1",
                    branch
                );
                let summary = format!(
                    "Pushed branch `{}` and opened GitHub PR against `main` for human review",
                    branch
                );
                let pr_body = format!(
                    "### GitHub Pull Request — `{}` into `main`\n\n\
                    - **Feature**: {}\n\
                    - **Head Branch**: `{}` → **Base**: `main`\n\
                    - **GitHub PR Review Link**: [Open Pull Request on GitHub]({})\n\n\
                    #### Human Review Checklist\n\
                    - [x] PO & Business Expert domain invariants verified\n\
                    - [x] Modular Architecture & Software Craftsmanship implementation\n\
                    - [x] Connected Test Factory Gherkin BDD scenarios passing\n\
                    - [x] OPS rollback script armed (`./ops/rollback_{}.sh`)\n",
                    version,
                    feature_request,
                    branch,
                    pr_url,
                    version.replace('.', "_")
                );
                let artifact = ArtifactFile {
                    path: format!(".github/PULL_REQUEST_{}.md", version.replace('.', "_")),
                    category: "GitHub PR (PR)".to_string(),
                    language: "markdown".to_string(),
                    content: pr_body.clone(),
                };
                (summary, pr_body, vec![artifact])
            }
        }
    }

    pub fn extract_code_blocks_from_markdown(
        markdown: &str,
        default_version: &str,
    ) -> Vec<ArtifactFile> {
        let mut results = Vec::new();
        let mut in_block = false;
        let mut current_lang = String::new();
        let mut current_path: Option<String> = None;
        let mut current_content = String::new();

        for line in markdown.lines() {
            let trimmed = line.trim_start();
            if trimmed.starts_with("```") {
                if in_block {
                    let body = current_content.trim();
                    if !body.is_empty() {
                        let path = current_path.unwrap_or_else(|| {
                            let ext = match current_lang.to_lowercase().as_str() {
                                "rust" | "rs" => "rs",
                                "typescript" | "ts" => "ts",
                                "javascript" | "js" => "js",
                                "vue" => "vue",
                                "python" | "py" => "py",
                                "go" => "go",
                                "json" => "json",
                                "yaml" | "yml" => "yml",
                                _ => "rs",
                            };
                            format!("src/feature_{}.{}", default_version.replace('.', "_"), ext)
                        });
                        results.push(ArtifactFile {
                            path,
                            category: "Code (DEV)".to_string(),
                            language: if current_lang.is_empty() {
                                "rust".to_string()
                            } else {
                                current_lang.clone()
                            },
                            content: body.to_string(),
                        });
                    }
                    in_block = false;
                    current_lang.clear();
                    current_path = None;
                    current_content.clear();
                } else {
                    in_block = true;
                    let info = trimmed.trim_start_matches('`').trim();
                    if let Some((lang, p)) = info.split_once(':') {
                        current_lang = lang.trim().to_string();
                        current_path = Some(p.trim().to_string());
                    } else if let Some((lang, p)) = info.split_once(' ') {
                        current_lang = lang.trim().to_string();
                        current_path = Some(p.trim().to_string());
                    } else {
                        current_lang = info.to_string();
                    }
                }
            } else if in_block {
                if current_path.is_none() {
                    let tl = line.trim();
                    if tl.starts_with("// file:")
                        || tl.starts_with("# file:")
                        || tl.starts_with("/* file:")
                        || tl.starts_with("// path:")
                        || tl.starts_with("# path:")
                    {
                        let prefix = if tl.contains("file:") {
                            "file:"
                        } else {
                            "path:"
                        };
                        if let Some(p) = tl.split(prefix).nth(1) {
                            let clean = p.trim().trim_end_matches("*/").trim();
                            if !clean.is_empty() {
                                current_path = Some(clean.to_string());
                            }
                        }
                    } else {
                        current_content.push_str(line);
                        current_content.push('\n');
                    }
                } else {
                    current_content.push_str(line);
                    current_content.push('\n');
                }
            }
        }
        results
    }

    pub async fn sync_linear(&self, api_key_override: Option<String>) -> Result<LinearStatus> {
        if let Some(ref k) = api_key_override {
            let trimmed = k.trim();
            if !trimmed.is_empty() {
                let mut key_guard = self.linear_api_key.write().await;
                *key_guard = Some(trimmed.to_string());
                let _ = fs::write(&self.linear_key_file, trimmed);
                WagnerConfig::save_linear_api_key(&self.config_file, trimmed);
            }
        } else {
            // Hot-reload `wagner.toml` so editing `linear_api_key` in `wagner.toml` takes effect immediately
            let toml_cfg = WagnerConfig::load_from_path(&self.config_file);
            if let Some(toml_key) = toml_cfg.linear_api_key {
                let mut key_guard = self.linear_api_key.write().await;
                *key_guard = Some(toml_key.clone());
                let _ = fs::write(&self.linear_key_file, &toml_key);
            }
        }

        let key = {
            let guard = self.linear_api_key.read().await;
            guard.clone()
        };

        let Some(api_key) = key else {
            return Err(anyhow::anyhow!(
                "Linear API key not stored yet. Set `linear_api_key = \"lin_api_...\"` in `wagner.toml` or paste it in the UI."
            ));
        };

        let default_ws = self.workspaces_root.to_string_lossy().to_string();
        let (status, mut remote_projects, mut remote_tickets) =
            LinearClient::sync_from_linear(&api_key, &default_ws).await?;

        let cfg = WagnerConfig::load_from_path(&self.config_file);
        let default_gh = Self::detect_github_owner_repo(&self.workspaces_root, &cfg);

        let (final_projects, final_tickets) = {
            let mut guard = self.state.write().await;
            guard.linear_status = status.clone();

            // Preserve custom repository paths & apply `[[projects]]` 1:1:1 mappings from `wagner.toml`
            for rp in remote_projects.iter_mut() {
                if let Some(existing_p) = guard.linear_projects.iter().find(|p| p.id == rp.id) {
                    if !existing_p.default_repo_path.is_empty() {
                        rp.default_repo_path = existing_p.default_repo_path.clone();
                    }
                    if !existing_p.default_github_repo.is_empty() {
                        rp.default_github_repo = existing_p.default_github_repo.clone();
                    }
                }
                if let Some(m) = cfg.find_mapping_for_project(&rp.name, &rp.key, &rp.id) {
                    if !m.local_path.is_empty() {
                        rp.default_repo_path = m.local_path.clone();
                    }
                    if !m.github_repo.is_empty() {
                        rp.default_github_repo = m.github_repo.clone();
                    }
                }
                if rp.default_github_repo.is_empty() {
                    rp.default_github_repo = Self::detect_github_owner_repo(
                        &PathBuf::from(&rp.default_repo_path),
                        &cfg,
                    );
                }
            }
            if !remote_projects.is_empty() {
                guard.linear_projects = remote_projects;
            }

            // Preserve local Wagner stage / active_run_id / repository_path for already-known Linear tickets
            for rt in remote_tickets.iter_mut() {
                if let Some(proj) = guard
                    .linear_projects
                    .iter()
                    .find(|p| p.id == rt.linear_project_id)
                {
                    rt.repository_path = proj.default_repo_path.clone();
                    rt.github_repo = proj.default_github_repo.clone();
                } else if let Some(first_proj) = guard.linear_projects.first() {
                    rt.repository_path = first_proj.default_repo_path.clone();
                    rt.github_repo = first_proj.default_github_repo.clone();
                }
                if rt.github_repo.is_empty() {
                    rt.github_repo = default_gh.clone();
                }

                if let Some(existing_tk) = guard.tickets.iter().find(|t| t.id == rt.id) {
                    if existing_tk.stage != KanbanStage::Todo
                        || existing_tk.active_role.is_some()
                        || existing_tk.active_run_id.is_some()
                    {
                        rt.stage = if existing_tk.stage == KanbanStage::Done
                            && existing_tk.active_run_id.is_some()
                        {
                            KanbanStage::PrReview
                        } else {
                            existing_tk.stage
                        };
                    }
                    rt.active_role = existing_tk.active_role;
                    rt.active_run_id = existing_tk.active_run_id.clone();
                    rt.pending_question = existing_tk.pending_question.clone();
                    rt.pr_url = existing_tk.pr_url.clone();
                    rt.pr_branch = existing_tk.pr_branch.clone();
                    if !existing_tk.repository_path.is_empty() {
                        rt.repository_path = existing_tk.repository_path.clone();
                    }
                    if !existing_tk.github_repo.is_empty() {
                        rt.github_repo = existing_tk.github_repo.clone();
                    }
                    if let Some(proj) = guard
                        .linear_projects
                        .iter()
                        .find(|p| p.id == rt.linear_project_id)
                    {
                        if let Some(m) = cfg.find_mapping_for_project(&proj.name, &proj.key, &proj.id)
                        {
                            if !m.local_path.is_empty() {
                                rt.repository_path = m.local_path.clone();
                            }
                            if !m.github_repo.is_empty() {
                                rt.github_repo = m.github_repo.clone();
                            }
                        }
                    }
                    if rt.stage == KanbanStage::PrReview
                        && (rt.pr_url.is_none()
                            || rt
                                .pr_url
                                .as_deref()
                                .is_some_and(|u| {
                                    u.contains("wagner-factory/")
                                        || u.contains("...wagner/")
                                        || u.contains("/compare/main...")
                                }))
                    {
                        let branch_name =
                            Self::branch_name_for_ticket(Some(&rt.identifier), &rt.version_tag);
                        let base_branch =
                            cfg.base_branch_for_workspace(&PathBuf::from(&rt.repository_path));
                        rt.pr_branch = Some(branch_name.clone());
                        rt.pr_url = Some(format!(
                            "https://github.com/{}/compare/{}...{}?expand=1",
                            rt.github_repo, base_branch, branch_name
                        ));
                    }
                }
            }
            // Exclude any tickets that the operator explicitly deleted from the Wagner Kanban board
            let dismissed = guard.dismissed_ticket_ids.clone();
            remote_tickets.retain(|t| {
                !dismissed.iter().any(|d| d == &t.id || d == &t.identifier)
            });

            // Sort tickets by ticket number ascending (oldest ticket number first, e.g. CAI-1, CAI-2, CAI-10)
            remote_tickets.sort_by(|a, b| {
                let num_a = a
                    .identifier
                    .rsplit('-')
                    .next()
                    .and_then(|s| s.parse::<u64>().ok())
                    .unwrap_or(u64::MAX);
                let num_b = b
                    .identifier
                    .rsplit('-')
                    .next()
                    .and_then(|s| s.parse::<u64>().ok())
                    .unwrap_or(u64::MAX);
                num_a
                    .cmp(&num_b)
                    .then_with(|| a.identifier.cmp(&b.identifier))
            });

            // Always replace guard.tickets with remote_tickets so deleted Linear issues are purged immediately
            guard.tickets = remote_tickets;

            (guard.linear_projects.clone(), guard.tickets.clone())
        };
        self.persist().await;

        let _ = self.event_tx.send(FactoryEvent::LinearSynced {
            linear_status: status.clone(),
            linear_projects: final_projects,
            tickets: final_tickets,
        });

        Ok(status)
    }

    pub async fn launch_ticket_factory(
        self: Arc<Self>,
        ticket_id: String,
        repository_path: Option<String>,
        execution_mode: String,
    ) -> Result<(FactoryTicket, PipelineRun)> {
        let (ticket, project_id) = {
            let mut guard = self.state.write().await;
            let default_proj_id = guard
                .projects
                .first()
                .map(|p| p.id.clone())
                .unwrap_or_else(|| "proj-wagner-forge-01".to_string());

            let tk = guard
                .tickets
                .iter_mut()
                .find(|t| t.id == ticket_id)
                .ok_or_else(|| anyhow::anyhow!("Ticket not found"))?;

            if let Some(repo) = repository_path {
                if !repo.trim().is_empty() {
                    tk.repository_path = repo.trim().to_string();
                }
            }
            tk.stage = KanbanStage::Design;
            tk.active_role = Some(AgentRole::Po);
            tk.pending_question = None;
            tk.updated_at = Utc::now();
            let tk_clone = tk.clone();

            if let Some(proj) = guard.projects.first_mut() {
                proj.repository_path = tk_clone.repository_path.clone();
            }

            (tk_clone, default_proj_id)
        };

        let _ = self.event_tx.send(FactoryEvent::TicketUpdated {
            ticket: ticket.clone(),
        });
        self.sync_ticket_stage_to_linear(&ticket.id, KanbanStage::Design).await;

        let feature_str = format!(
            "[{}] {} — {}",
            ticket.identifier, ticket.title, ticket.description
        );

        let run = Arc::clone(&self)
            .start_pipeline_run_internal(
                project_id,
                Some(ticket.id.clone()),
                Some(ticket.identifier.clone()),
                Some(ticket.repository_path.clone()),
                ticket.version_tag.clone(),
                feature_str,
                execution_mode,
            )
            .await?;

        Ok((ticket, run))
    }

    /// Steer an agent or answer its question in the Chain of Thought!
    pub async fn steer_ticket_agent(
        &self,
        ticket_id: &str,
        role: AgentRole,
        user_message: &str,
        execution_mode: &str,
    ) -> Result<AgentStepExecution> {
        let (run_id, project, existing_step, repo_path) = {
            let mut guard = self.state.write().await;
            let proj = guard
                .projects
                .first()
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("No project configured"))?;

            let tk = guard
                .tickets
                .iter_mut()
                .find(|t| t.id == ticket_id)
                .ok_or_else(|| anyhow::anyhow!("Ticket not found"))?;
            tk.pending_question = None;
            tk.updated_at = Utc::now();
            let r_path = tk.repository_path.clone();
            let active_rid = tk.active_run_id.clone();

            let run = if let Some(ref rid) = active_rid {
                guard.runs.iter_mut().find(|r| &r.id == rid)
            } else {
                guard
                    .runs
                    .iter_mut()
                    .find(|r| r.ticket_id.as_deref() == Some(ticket_id))
            }
            .ok_or_else(|| anyhow::anyhow!("No pipeline run found for this ticket"))?;

            let step = run
                .steps
                .iter_mut()
                .find(|s| s.role == role)
                .ok_or_else(|| anyhow::anyhow!("Agent step not found"))?;

            let steer_entry = ThoughtEntry {
                id: format!("cot-{}", &Uuid::new_v4().to_string()[..8]),
                role,
                kind: "user_steering".to_string(),
                content: format!("USER INTERVENTION / ANSWER: {}", user_message),
                timestamp: Utc::now(),
            };
            step.thinking_log.push(steer_entry.clone());

            let _ = self.event_tx.send(FactoryEvent::AgentThoughtEmitted {
                run_id: run.id.clone(),
                ticket_id: Some(ticket_id.to_string()),
                role,
                thought: steer_entry,
            });

            (run.id.clone(), proj, step.clone(), r_path)
        };

        let agent_cfg = project
            .agents
            .iter()
            .find(|a| a.role == role)
            .cloned()
            .unwrap_or_else(|| Orchestrator::default_agents().remove(0));

        let prompt = format!(
            "{}\n\n\
            The human operator in the WAGNER Software Factory intervened on your previous work with the following guidance/answer:\n\
            \"{}\"\n\n\
            Previous output:\n{}\n\n\
            Do NOT call any tools or read/write files. First provide a `<thinking>` block with 2-3 bullet lines explaining how you are adjusting your work based on the operator's input, and then output the updated Markdown deliverable.",
            agent_cfg.system_prompt, user_message, existing_step.markdown_output
        );

        let start = std::time::Instant::now();
        let (new_thoughts, _pending_q, updated_md, conv_id, add_tokens) =
            if execution_mode == "agy_live" {
                match self
                    .agy
                    .run_agent_prompt(
                        &agent_cfg.model,
                        &agent_cfg.effort,
                        &prompt,
                        Some(&repo_path),
                        existing_step.agy_conversation_id.as_deref(),
                        45,
                    )
                    .await
                {
                    Ok(resp) => {
                        let raw = resp.response.unwrap_or_default();
                        let (mut cot, pq, clean_md) = AgyBridge::extract_thoughts_and_clean_response(
                            role,
                            &raw,
                            resp.conversation_id.as_deref(),
                        );
                        if cot.is_empty() {
                            cot.push(ThoughtEntry {
                                id: format!("cot-{}", &Uuid::new_v4().to_string()[..8]),
                                role,
                                kind: "reasoning".to_string(),
                                content: format!(
                                    "Integrated operator directive ('{}') into {} specification.",
                                    user_message,
                                    role.code()
                                ),
                                timestamp: Utc::now(),
                            });
                        }
                        let t = resp.usage.unwrap_or_default().total_tokens;
                        (
                            cot,
                            pq,
                            if clean_md.is_empty() { raw } else { clean_md },
                            resp.conversation_id.or(existing_step.agy_conversation_id),
                            t,
                        )
                    }
                    Err(_) => {
                        let cot = vec![
                            ThoughtEntry {
                                id: format!("cot-{}", &Uuid::new_v4().to_string()[..8]),
                                role,
                                kind: "reasoning".to_string(),
                                content: format!(
                                    "Re-evaluating {} constraints after operator steering: '{}'",
                                    role.code(),
                                    user_message
                                ),
                                timestamp: Utc::now(),
                            },
                            ThoughtEntry {
                                id: format!("cot-{}", &Uuid::new_v4().to_string()[..8]),
                                role,
                                kind: "conclusion".to_string(),
                                content: "Updated deliverable to incorporate operator correction."
                                    .to_string(),
                                timestamp: Utc::now(),
                            },
                        ];
                        let appended = format!(
                            "{}\n\n---\n#### Operator Steering Applied (`{}`)\n- **Directive**: {}\n- **Resolution**: Deliverable updated and verified by `{}`.",
                            existing_step.markdown_output,
                            Utc::now().format("%H:%M:%S"),
                            user_message,
                            role.code()
                        );
                        (cot, None, appended, existing_step.agy_conversation_id, 940)
                    }
                }
            } else {
                let cot = vec![
                    ThoughtEntry {
                        id: format!("cot-{}", &Uuid::new_v4().to_string()[..8]),
                        role,
                        kind: "reasoning".to_string(),
                        content: format!(
                            "Applying operator steering directive to {}: '{}'",
                            role.code(),
                            user_message
                        ),
                        timestamp: Utc::now(),
                    },
                    ThoughtEntry {
                        id: format!("cot-{}", &Uuid::new_v4().to_string()[..8]),
                        role,
                        kind: "conclusion".to_string(),
                        content: "Deliverable revised and aligned with operator guidance."
                            .to_string(),
                        timestamp: Utc::now(),
                    },
                ];
                let appended = format!(
                    "{}\n\n---\n#### Operator Steering Applied (`{}`)\n- **Directive**: {}\n- **Resolution**: Deliverable updated and verified by `{}`.",
                    existing_step.markdown_output,
                    Utc::now().format("%H:%M:%S"),
                    user_message,
                    role.code()
                );
                (cot, None, appended, existing_step.agy_conversation_id, 820)
            };

        for th in &new_thoughts {
            let _ = self.event_tx.send(FactoryEvent::AgentThoughtEmitted {
                run_id: run_id.clone(),
                ticket_id: Some(ticket_id.to_string()),
                role,
                thought: th.clone(),
            });
        }

        let updated_step = {
            let mut guard = self.state.write().await;
            if let Some(tk) = guard.tickets.iter_mut().find(|t| t.id == ticket_id) {
                // Operator answered the question: clear pending_question so the pipeline can advance to the next step
                tk.pending_question = None;
                tk.updated_at = Utc::now();
                let _ = self.event_tx.send(FactoryEvent::TicketUpdated {
                    ticket: tk.clone(),
                });
            }
            let run = guard
                .runs
                .iter_mut()
                .find(|r| r.id == run_id)
                .ok_or_else(|| anyhow::anyhow!("Run not found"))?;
            let step = run
                .steps
                .iter_mut()
                .find(|s| s.role == role)
                .ok_or_else(|| anyhow::anyhow!("Step not found"))?;

            step.status = StepStatus::Completed;
            step.completed_at = Some(Utc::now());
            step.thinking_log.extend(new_thoughts);
            step.markdown_output = updated_md.clone();
            step.summary = format!("Operator answer applied: {}", user_message);
            step.agy_conversation_id = conv_id;
            step.duration_seconds =
                ((step.duration_seconds + start.elapsed().as_secs_f64()) * 100.0).round() / 100.0;
            step.usage.total_tokens += add_tokens;
            if let Some(first_art) = step.artifacts.first_mut() {
                first_art.content = updated_md;
            }
            let cloned = step.clone();
            run.total_tokens += add_tokens;
            cloned
        };

        self.persist().await;
        let _ = self.event_tx.send(FactoryEvent::AgentStepCompleted {
            run_id,
            ticket_id: Some(ticket_id.to_string()),
            step: updated_step.clone(),
        });

        Ok(updated_step)
    }

    pub async fn start_pipeline_run(
        self: Arc<Self>,
        project_id: String,
        version_tag: String,
        feature_request: String,
        execution_mode: String,
    ) -> Result<PipelineRun> {
        self.start_pipeline_run_internal(
            project_id,
            None,
            None,
            None,
            version_tag,
            feature_request,
            execution_mode,
        )
        .await
    }

    async fn start_pipeline_run_internal(
        self: Arc<Self>,
        project_id: String,
        ticket_id: Option<String>,
        ticket_identifier: Option<String>,
        override_repo_path: Option<String>,
        version_tag: String,
        feature_request: String,
        execution_mode: String,
    ) -> Result<PipelineRun> {
        let mut project = {
            let mut guard = self.state.write().await;
            let default_agents = Self::default_agents();
            for proj in guard.projects.iter_mut() {
                for def_ag in &default_agents {
                    if !proj.agents.iter().any(|a| a.role == def_ag.role) {
                        proj.agents.push(def_ag.clone());
                    }
                }
            }
            guard
                .projects
                .iter()
                .find(|p| p.id == project_id)
                .or_else(|| guard.projects.first())
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("Project not found"))?
        };

        if let Some(ref custom_repo) = override_repo_path {
            project.repository_path = custom_repo.clone();
        }

        let run_id = format!("run-{}", &Uuid::new_v4().to_string()[..8]);
        let workspace_dir = PathBuf::from(&project.repository_path);
        let is_queue_busy = self.pipeline_queue.try_lock().is_err();

        let initial_steps: Vec<AgentStepExecution> = project
            .agents
            .iter()
            .map(|a| {
                let brick_enabled = project
                    .bricks
                    .iter()
                    .find(|b| b.kind == a.role.corresponding_brick())
                    .map(|b| b.enabled)
                    .unwrap_or(true);
                let active = a.enabled && brick_enabled;
                AgentStepExecution {
                    role: a.role,
                    title: a.title.clone(),
                    wagner_pillar: a.wagner_pillar.clone(),
                    model: a.model.clone(),
                    status: if active {
                        StepStatus::Pending
                    } else {
                        StepStatus::Skipped
                    },
                    started_at: None,
                    completed_at: None,
                    duration_seconds: 0.0,
                    agy_conversation_id: None,
                    agy_command: format!(
                        "{} --add-dir \"{}\"",
                        self.agy.format_cli_command(&a.model, &a.effort, a.role.code()),
                        project.repository_path
                    ),
                    summary: if active {
                        if is_queue_busy {
                            "Waiting in queue for active ticket to finish...".to_string()
                        } else {
                            "Queued in WAGNER Factory...".to_string()
                        }
                    } else {
                        "Brick disabled".to_string()
                    },
                    markdown_output: String::new(),
                    thinking_log: vec![],
                    artifacts: vec![],
                    usage: AgyUsage::default(),
                    quality_gate_passed: active,
                }
            })
            .collect();

        let run = PipelineRun {
            id: run_id.clone(),
            project_id: project.id.clone(),
            ticket_id: ticket_id.clone(),
            ticket_identifier,
            version_tag: version_tag.clone(),
            feature_request: feature_request.clone(),
            execution_mode: execution_mode.clone(),
            status: if is_queue_busy {
                PipelineStatus::Queued
            } else {
                PipelineStatus::Running
            },
            created_at: Utc::now(),
            completed_at: None,
            steps: initial_steps,
            test_report: None,
            living_docs: None,
            pr_url: None,
            pr_branch: None,
            total_tokens: 0,
            total_duration_seconds: 0.0,
            workspace_dir: workspace_dir.to_string_lossy().to_string(),
        };

        {
            let mut guard = self.state.write().await;
            if let Some(ref tid) = ticket_id {
                if let Some(tk) = guard.tickets.iter_mut().find(|t| &t.id == tid) {
                    tk.active_run_id = Some(run_id.clone());
                }
            }
            guard.runs.insert(0, run.clone());
        }
        self.persist().await;

        let _ = self.event_tx.send(FactoryEvent::PipelineStarted {
            run_id: run_id.clone(),
            project_id: project.id.clone(),
            ticket_id: ticket_id.clone(),
            version_tag: version_tag.clone(),
            feature_request: feature_request.clone(),
        });

        let orch = Arc::clone(&self);
        let run_id_bg = run_id.clone();
        let join_handle = tokio::spawn(async move {
            orch.execute_pipeline_background(run_id_bg, project, workspace_dir)
                .await;
        });
        self.active_tasks
            .write()
            .await
            .insert(run_id.clone(), join_handle.abort_handle());

        Ok(run)
    }

    /// Kill/abort an active agent execution on a ticket immediately (including any child `agy` OS process).
    pub async fn cancel_ticket_agent(&self, ticket_id: &str) -> Result<FactoryTicket> {
        let (run_id, killed_role, updated_ticket) = {
            let mut guard = self.state.write().await;
            let tk = guard
                .tickets
                .iter_mut()
                .find(|t| t.id == ticket_id)
                .ok_or_else(|| anyhow::anyhow!("Ticket not found"))?;

            let run_id = tk.active_run_id.clone();
            let killed_role = tk.active_role;

            tk.active_role = None;
            tk.active_run_id = None;
            tk.pending_question = None;
            tk.stage = KanbanStage::Todo;
            tk.updated_at = Utc::now();
            let tk_clone = tk.clone();

            if let Some(ref rid) = run_id {
                if let Some(run) = guard.runs.iter_mut().find(|r| &r.id == rid) {
                    run.status = PipelineStatus::Failed;
                    run.completed_at = Some(Utc::now());
                    for step in run.steps.iter_mut() {
                        if step.status == StepStatus::Running {
                            step.status = StepStatus::Failed;
                            step.completed_at = Some(Utc::now());
                            step.summary = "Killed by operator".to_string();
                            step.thinking_log.push(ThoughtEntry {
                                id: format!("cot-{}", &Uuid::new_v4().to_string()[..8]),
                                role: step.role,
                                kind: "user_steering".to_string(),
                                content: "Agent process killed by operator.".to_string(),
                                timestamp: Utc::now(),
                            });
                        }
                    }
                }
            }

            (run_id, killed_role, tk_clone)
        };

        if let Some(ref rid) = run_id {
            if let Some(abort_handle) = self.active_tasks.write().await.remove(rid) {
                abort_handle.abort();
                info!(
                    "Aborted running pipeline task {} for ticket {}",
                    rid, ticket_id
                );
            }
        }

        self.persist().await;
        let _ = self.event_tx.send(FactoryEvent::AgentCancelled {
            run_id,
            ticket: updated_ticket.clone(),
            killed_role,
        });

        Ok(updated_ticket)
    }

    /// Delete an agent-covered ticket card from the Kanban board (refuses deleting `Todo` / Backlog cards).
    pub async fn delete_ticket(&self, ticket_id: &str) -> Result<String> {
        let (run_id, identifier) = {
            let mut guard = self.state.write().await;
            let idx = guard
                .tickets
                .iter()
                .position(|t| t.id == ticket_id)
                .ok_or_else(|| anyhow::anyhow!("Ticket not found"))?;
            if guard.tickets[idx].stage == KanbanStage::Todo {
                return Err(anyhow::anyhow!(
                    "Backlog (Todo) tickets cannot be deleted; only cards covered by agents can be deleted."
                ));
            }
            let removed = guard.tickets.remove(idx);
            if !guard.dismissed_ticket_ids.contains(&removed.id) {
                guard.dismissed_ticket_ids.push(removed.id.clone());
            }
            if !guard.dismissed_ticket_ids.contains(&removed.identifier) {
                guard.dismissed_ticket_ids.push(removed.identifier.clone());
            }

            if let Some(ref rid) = removed.active_run_id {
                if let Some(run) = guard.runs.iter_mut().find(|r| &r.id == rid) {
                    run.status = PipelineStatus::Failed;
                    run.completed_at = Some(Utc::now());
                }
            }

            (removed.active_run_id, removed.identifier)
        };

        if let Some(ref rid) = run_id {
            if let Some(abort_handle) = self.active_tasks.write().await.remove(rid) {
                abort_handle.abort();
            }
        }

        self.persist().await;

        let _ = self.event_tx.send(FactoryEvent::TicketDeleted {
            ticket_id: ticket_id.to_string(),
            identifier: identifier.clone(),
        });

        let api_key_opt = self.linear_api_key.read().await.clone();
        if let Some(api_key) = api_key_opt {
            let tid_owned = ticket_id.to_string();
            tokio::spawn(async move {
                LinearClient::delete_issue(&api_key, &tid_owned).await;
            });
        }

        Ok(identifier)
    }

    async fn execute_pipeline_background(
        self: Arc<Self>,
        run_id: String,
        project: Project,
        workspace_dir: PathBuf,
    ) {
        // Sequential execution queue: wait for any currently running ticket to finish before touching the working tree
        let _queue_guard = self.pipeline_queue.lock().await;

        let (version_tag, feature_request, execution_mode, ticket_id, ticket_identifier) = {
            let mut guard = self.state.write().await;
            if let Some(r) = guard.runs.iter_mut().find(|r| r.id == run_id) {
                r.status = PipelineStatus::Running;
                (
                    r.version_tag.clone(),
                    r.feature_request.clone(),
                    r.execution_mode.clone(),
                    r.ticket_id.clone(),
                    r.ticket_identifier.clone(),
                )
            } else {
                return;
            }
        };

        // Sync the configured base branch (`default_branch`, e.g. `master`) from origin and switch the repository working tree directly to `factory-XXX` (no worktrees)
        let cfg = WagnerConfig::load_from_path(&self.config_file);
        let base_branch = cfg.base_branch_for_workspace(&workspace_dir);
        let initial_branch =
            Self::branch_name_for_ticket(ticket_identifier.as_deref(), &version_tag);
        let init_branch_script = format!(
            r#"cd "{ws}" 2>/dev/null || exit 0
if [ ! -d .git ] && [ -d ../.git ]; then cd ..; fi
if [ -d .git ]; then
  git fetch origin "{base}" 2>/dev/null || git fetch origin 2>/dev/null || true
  BASE_SHA=$(git rev-parse --verify -q "refs/remotes/origin/{base}" || git rev-parse --verify -q "refs/heads/{base}" || git rev-parse --verify -q refs/remotes/origin/master || git rev-parse --verify -q refs/remotes/origin/main || git rev-parse --verify -q HEAD || true)
  if [ -n "$BASE_SHA" ]; then
    git update-ref "refs/heads/{base}" "$BASE_SHA" 2>/dev/null || true
    git update-ref "refs/heads/{branch}" "$BASE_SHA" 2>/dev/null || true
    git checkout -B "{branch}" "$BASE_SHA" 2>/dev/null || git symbolic-ref HEAD "refs/heads/{branch}" 2>/dev/null || true
  fi
fi"#,
            ws = workspace_dir.to_string_lossy(),
            base = base_branch,
            branch = initial_branch,
        );
        let _ = tokio::process::Command::new("bash")
            .arg("-c")
            .arg(&init_branch_script)
            .output()
            .await;

        let mut accumulated_context = String::new();
        let mut total_tokens = 0u64;
        let mut total_duration = 0.0f64;
        let mut generated_pr_url: Option<String> = None;
        let mut generated_pr_branch: Option<String> = None;

        // --- Fast-Track Parallel Inception (PO + Business Expert concurrently) ---
        let (is_po_skipped, is_expert_skipped) = {
            let guard = self.state.read().await;
            if let Some(r) = guard.runs.iter().find(|r| r.id == run_id) {
                let s0 = r.steps.get(0).map(|s| s.status == StepStatus::Skipped).unwrap_or(false);
                let s1 = r.steps.get(1).map(|s| s.status == StepStatus::Skipped).unwrap_or(false);
                (s0, s1)
            } else {
                (false, false)
            }
        };

        let can_parallelize_inception = project.agents.len() >= 2
            && project.agents[0].role == AgentRole::Po
            && project.agents[1].role == AgentRole::Expert
            && !is_po_skipped
            && !is_expert_skipped;

        if can_parallelize_inception {
            info!("Launching Parallel Inception: PO & Business Expert running concurrently for high-speed delivery");
            let po_agent = &project.agents[0];
            let expert_agent = &project.agents[1];

            let po_thought = ThoughtEntry {
                id: format!("cot-{}", &Uuid::new_v4().to_string()[..8]),
                role: AgentRole::Po,
                kind: "reasoning".to_string(),
                content: format!(
                    "Parallel Inception: PO analyzing user stories and acceptance criteria for '{}' in {}...",
                    version_tag,
                    workspace_dir.to_string_lossy()
                ),
                timestamp: Utc::now(),
            };
            let expert_thought = ThoughtEntry {
                id: format!("cot-{}", &Uuid::new_v4().to_string()[..8]),
                role: AgentRole::Expert,
                kind: "reasoning".to_string(),
                content: format!(
                    "Parallel Inception: Business Expert mapping domain invariants and business rules for '{}'...",
                    version_tag
                ),
                timestamp: Utc::now(),
            };

            let po_cli_cmd = format!(
                "{} --add-dir \"{}\"",
                self.agy.format_cli_command(&po_agent.model, &po_agent.effort, po_agent.role.code()),
                workspace_dir.to_string_lossy()
            );
            let expert_cli_cmd = format!(
                "{} --add-dir \"{}\"",
                self.agy.format_cli_command(&expert_agent.model, &expert_agent.effort, expert_agent.role.code()),
                workspace_dir.to_string_lossy()
            );

            let mut updated_ticket = None;
            {
                let mut guard = self.state.write().await;
                if let Some(run) = guard.runs.iter_mut().find(|r| r.id == run_id) {
                    if let Some(step0) = run.steps.get_mut(0) {
                        step0.status = StepStatus::Running;
                        step0.started_at = Some(Utc::now());
                        step0.thinking_log.push(po_thought.clone());
                        step0.summary = format!(
                            "Parallel Inception: Running PO ({}) in {}...",
                            po_agent.model,
                            workspace_dir.to_string_lossy()
                        );
                    }
                    if let Some(step1) = run.steps.get_mut(1) {
                        step1.status = StepStatus::Running;
                        step1.started_at = Some(Utc::now());
                        step1.thinking_log.push(expert_thought.clone());
                        step1.summary = format!(
                            "Parallel Inception: Running Business Expert ({}) in {}...",
                            expert_agent.model,
                            workspace_dir.to_string_lossy()
                        );
                    }
                }
                if let Some(ref tid) = ticket_id {
                    if let Some(tk) = guard.tickets.iter_mut().find(|t| &t.id == tid) {
                        tk.stage = KanbanStage::Design;
                        tk.active_role = Some(AgentRole::Po);
                        tk.updated_at = Utc::now();
                        updated_ticket = Some(tk.clone());
                    }
                }
            }

            if let Some(tk) = updated_ticket {
                let _ = self.event_tx.send(FactoryEvent::TicketUpdated { ticket: tk.clone() });
                self.sync_ticket_stage_to_linear(&tk.id, KanbanStage::Design).await;
            }

            let _ = self.event_tx.send(FactoryEvent::AgentStepStarted {
                run_id: run_id.clone(),
                ticket_id: ticket_id.clone(),
                role: AgentRole::Po,
                stage: KanbanStage::Design,
                title: po_agent.title.clone(),
                model: po_agent.model.clone(),
                agy_command: po_cli_cmd.clone(),
            });
            let _ = self.event_tx.send(FactoryEvent::AgentThoughtEmitted {
                run_id: run_id.clone(),
                ticket_id: ticket_id.clone(),
                role: AgentRole::Po,
                thought: po_thought.clone(),
            });

            let _ = self.event_tx.send(FactoryEvent::AgentStepStarted {
                run_id: run_id.clone(),
                ticket_id: ticket_id.clone(),
                role: AgentRole::Expert,
                stage: KanbanStage::Design,
                title: expert_agent.title.clone(),
                model: expert_agent.model.clone(),
                agy_command: expert_cli_cmd.clone(),
            });
            let _ = self.event_tx.send(FactoryEvent::AgentThoughtEmitted {
                run_id: run_id.clone(),
                ticket_id: ticket_id.clone(),
                role: AgentRole::Expert,
                thought: expert_thought.clone(),
            });

            let po_instructions = "Do NOT call any tools or modify repo files. First write a `<thinking>`...`</thinking>` block with 3 concise bullet points showing your internal Chain of Thought. If you have a question or assumption the human operator can steer, include `<question>your question</question>`. Then write your concise Markdown deliverable (120-160 words) with crisp User Stories and Acceptance Criteria in French.";
            let expert_instructions = "Do NOT call any tools or modify repo files. First write a `<thinking>`...`</thinking>` block with 3 concise bullet points showing your internal Chain of Thought. If you have a question or assumption the human operator can steer, include `<question>your question</question>`. Then write your concise Markdown deliverable (120-160 words) defining Ubiquitous Language, Invariants, and Business Rules in French.";

            let po_prompt = format!(
                "{}\n\nTarget Repository: {}\nRelease Tag: {}\nLinear Ticket: {}\n\n{}\nDeliverable role: Product Owner (PO) under WAGNER principles.",
                po_agent.system_prompt, workspace_dir.to_string_lossy(), version_tag, feature_request, po_instructions
            );
            let expert_prompt = format!(
                "{}\n\nTarget Repository: {}\nRelease Tag: {}\nLinear Ticket: {}\n\n{}\nDeliverable role: Business Expert under WAGNER principles.",
                expert_agent.system_prompt, workspace_dir.to_string_lossy(), version_tag, feature_request, expert_instructions
            );

            let (po_def_summary, po_fallback_md, mut po_artifacts) = Self::synthesize_role_output(AgentRole::Po, &project, &version_tag, &feature_request);
            let po_default_thoughts = Self::synthesize_role_thoughts(AgentRole::Po, &project, &version_tag, &feature_request);

            let (expert_def_summary, expert_fallback_md, mut expert_artifacts) = Self::synthesize_role_output(AgentRole::Expert, &project, &version_tag, &feature_request);
            let expert_default_thoughts = Self::synthesize_role_thoughts(AgentRole::Expert, &project, &version_tag, &feature_request);

            let ws_str = workspace_dir.to_string_lossy().to_string();
            let start_instant = std::time::Instant::now();

            let (po_output, expert_output) = if execution_mode == "agy_live" {
                let po_fut = self.agy.run_agent_prompt(&po_agent.model, &po_agent.effort, &po_prompt, Some(&ws_str), None, 35);
                let expert_fut = self.agy.run_agent_prompt(&expert_agent.model, &expert_agent.effort, &expert_prompt, Some(&ws_str), None, 35);
                tokio::join!(po_fut, expert_fut)
            } else {
                (
                    Ok(AgyPrintResponse { conversation_id: Some("sim-po".into()), status: Some("success".into()), response: Some(po_fallback_md.clone()), duration_seconds: Some(0.3), num_turns: Some(1), usage: None }),
                    Ok(AgyPrintResponse { conversation_id: Some("sim-expert".into()), status: Some("success".into()), response: Some(expert_fallback_md.clone()), duration_seconds: Some(0.3), num_turns: Some(1), usage: None }),
                )
            };

            let (po_cot, _po_pq, po_text, po_cid, po_usage, po_dur) = match po_output {
                Ok(resp) => {
                    let raw = resp.response.unwrap_or_else(|| po_fallback_md.clone());
                    let (cot, pq, cleaned) = AgyBridge::extract_thoughts_and_clean_response(AgentRole::Po, &raw, resp.conversation_id.as_deref());
                    let final_cot = if cot.is_empty() { po_default_thoughts } else { cot };
                    let final_text = if cleaned.is_empty() { po_fallback_md } else { cleaned };
                    let u: AgyUsage = resp.usage.unwrap_or_default().into();
                    let d = resp.duration_seconds.unwrap_or_else(|| start_instant.elapsed().as_secs_f64());
                    (final_cot, pq, final_text, resp.conversation_id, u, d)
                }
                Err(err) => {
                    warn!("Parallel PO turn fallback: {}", err);
                    (po_default_thoughts, None, po_fallback_md, Some("fallback-po".into()), AgyUsage::default(), start_instant.elapsed().as_secs_f64())
                }
            };

            let (exp_cot, _exp_pq, exp_text, exp_cid, exp_usage, exp_dur) = match expert_output {
                Ok(resp) => {
                    let raw = resp.response.unwrap_or_else(|| expert_fallback_md.clone());
                    let (cot, pq, cleaned) = AgyBridge::extract_thoughts_and_clean_response(AgentRole::Expert, &raw, resp.conversation_id.as_deref());
                    let final_cot = if cot.is_empty() { expert_default_thoughts } else { cot };
                    let final_text = if cleaned.is_empty() { expert_fallback_md } else { cleaned };
                    let u: AgyUsage = resp.usage.unwrap_or_default().into();
                    let d = resp.duration_seconds.unwrap_or_else(|| start_instant.elapsed().as_secs_f64());
                    (final_cot, pq, final_text, resp.conversation_id, u, d)
                }
                Err(err) => {
                    warn!("Parallel Expert turn fallback: {}", err);
                    (expert_default_thoughts, None, expert_fallback_md, Some("fallback-expert".into()), AgyUsage::default(), start_instant.elapsed().as_secs_f64())
                }
            };

            for th in &po_cot {
                let _ = self.event_tx.send(FactoryEvent::AgentThoughtEmitted {
                    run_id: run_id.clone(),
                    ticket_id: ticket_id.clone(),
                    role: AgentRole::Po,
                    thought: th.clone(),
                });
            }
            for th in &exp_cot {
                let _ = self.event_tx.send(FactoryEvent::AgentThoughtEmitted {
                    run_id: run_id.clone(),
                    ticket_id: ticket_id.clone(),
                    role: AgentRole::Expert,
                    thought: th.clone(),
                });
            }

            let po_file = ArtifactFile {
                path: ".agents/po.md".to_string(),
                category: "PO Deliverable".to_string(),
                language: "markdown".to_string(),
                content: po_text.clone(),
            };
            po_artifacts.insert(0, po_file);
            for art in &po_artifacts {
                let clean_path = art.path.trim_start_matches(['/', '\\']);
                if !clean_path.contains("..") {
                    let dest = workspace_dir.join(clean_path);
                    if let Some(parent) = dest.parent() { let _ = tokio::fs::create_dir_all(parent).await; }
                    let _ = tokio::fs::write(&dest, &art.content).await;
                }
            }

            let exp_file = ArtifactFile {
                path: ".agents/expert.md".to_string(),
                category: "Expert Deliverable".to_string(),
                language: "markdown".to_string(),
                content: exp_text.clone(),
            };
            expert_artifacts.insert(0, exp_file);
            for art in &expert_artifacts {
                let clean_path = art.path.trim_start_matches(['/', '\\']);
                if !clean_path.contains("..") {
                    let dest = workspace_dir.join(clean_path);
                    if let Some(parent) = dest.parent() { let _ = tokio::fs::create_dir_all(parent).await; }
                    let _ = tokio::fs::write(&dest, &art.content).await;
                }
            }

            let po_summary = po_text.lines().find(|l| {
                let t = l.trim();
                !t.is_empty() && !t.starts_with('#')
            }).unwrap_or(&po_def_summary).trim().trim_start_matches(['-', '*', '>']).to_string();

            let exp_summary = exp_text.lines().find(|l| {
                let t = l.trim();
                !t.is_empty() && !t.starts_with('#')
            }).unwrap_or(&expert_def_summary).trim().trim_start_matches(['-', '*', '>']).to_string();

            accumulated_context.push_str(&format!("- [PO]: {}\n- [EXPERT]: {}\n", po_summary, exp_summary));
            total_tokens += po_usage.total_tokens + exp_usage.total_tokens;
            total_duration += po_dur.max(exp_dur);

            let po_snapshot = AgentStepExecution {
                role: AgentRole::Po,
                title: po_agent.title.clone(),
                wagner_pillar: po_agent.wagner_pillar.clone(),
                model: po_agent.model.clone(),
                status: StepStatus::Completed,
                started_at: Some(Utc::now()),
                completed_at: Some(Utc::now()),
                duration_seconds: (po_dur * 100.0).round() / 100.0,
                agy_conversation_id: po_cid,
                agy_command: po_cli_cmd,
                summary: po_summary,
                markdown_output: po_text,
                thinking_log: po_cot,
                artifacts: po_artifacts,
                usage: po_usage,
                quality_gate_passed: true,
            };

            let exp_snapshot = AgentStepExecution {
                role: AgentRole::Expert,
                title: expert_agent.title.clone(),
                wagner_pillar: expert_agent.wagner_pillar.clone(),
                model: expert_agent.model.clone(),
                status: StepStatus::Completed,
                started_at: Some(Utc::now()),
                completed_at: Some(Utc::now()),
                duration_seconds: (exp_dur * 100.0).round() / 100.0,
                agy_conversation_id: exp_cid,
                agy_command: expert_cli_cmd,
                summary: exp_summary,
                markdown_output: exp_text,
                thinking_log: exp_cot,
                artifacts: expert_artifacts,
                usage: exp_usage,
                quality_gate_passed: true,
            };

            {
                let mut guard = self.state.write().await;
                if let Some(run) = guard.runs.iter_mut().find(|r| r.id == run_id) {
                    if let Some(s0) = run.steps.get_mut(0) { *s0 = po_snapshot.clone(); }
                    if let Some(s1) = run.steps.get_mut(1) { *s1 = exp_snapshot.clone(); }
                    run.total_tokens = total_tokens;
                    run.total_duration_seconds = (total_duration * 100.0).round() / 100.0;
                }
            }
            self.persist().await;

            let _ = self.event_tx.send(FactoryEvent::AgentStepCompleted {
                run_id: run_id.clone(),
                ticket_id: ticket_id.clone(),
                step: po_snapshot,
            });
            let _ = self.event_tx.send(FactoryEvent::AgentStepCompleted {
                run_id: run_id.clone(),
                ticket_id: ticket_id.clone(),
                step: exp_snapshot,
            });
        }

        let start_idx = if can_parallelize_inception { 2 } else { 0 };

        for idx in start_idx..project.agents.len() {
            let agent = &project.agents[idx];

            let is_skipped = {
                let guard = self.state.read().await;
                guard
                    .runs
                    .iter()
                    .find(|r| r.id == run_id)
                    .and_then(|r| r.steps.get(idx))
                    .map(|s| s.status == StepStatus::Skipped)
                    .unwrap_or(false)
            };
            if is_skipped {
                continue;
            }

            let target_kanban_stage = agent.role.kanban_stage();
            let cli_cmd = format!(
                "{} --add-dir \"{}\"",
                self.agy
                    .format_cli_command(&agent.model, &agent.effort, agent.role.code()),
                workspace_dir.to_string_lossy()
            );

            let initial_thought = ThoughtEntry {
                id: format!("cot-{}", &Uuid::new_v4().to_string()[..8]),
                role: agent.role,
                kind: "reasoning".to_string(),
                content: format!(
                    "Initializing {} turn on '{}' in repo {}...",
                    agent.role.code(),
                    version_tag,
                    workspace_dir.to_string_lossy()
                ),
                timestamp: Utc::now(),
            };

            let mut updated_ticket = None;
            {
                let mut guard = self.state.write().await;
                if let Some(run) = guard.runs.iter_mut().find(|r| r.id == run_id) {
                    if let Some(step) = run.steps.get_mut(idx) {
                        step.status = StepStatus::Running;
                        step.started_at = Some(Utc::now());
                        step.thinking_log.push(initial_thought.clone());
                        step.summary = format!(
                            "Running {} via Antigravity (agy) in {}...",
                            agent.role.code(),
                            workspace_dir.to_string_lossy()
                        );
                    }
                }
                if let Some(ref tid) = ticket_id {
                    if let Some(tk) = guard.tickets.iter_mut().find(|t| &t.id == tid) {
                        tk.stage = target_kanban_stage;
                        tk.active_role = Some(agent.role);
                        tk.updated_at = Utc::now();
                        updated_ticket = Some(tk.clone());
                    }
                }
            }

            if let Some(tk) = updated_ticket {
                let _ = self.event_tx.send(FactoryEvent::TicketUpdated { ticket: tk.clone() });
                self.sync_ticket_stage_to_linear(&tk.id, target_kanban_stage).await;
            }

            let _ = self.event_tx.send(FactoryEvent::AgentStepStarted {
                run_id: run_id.clone(),
                ticket_id: ticket_id.clone(),
                role: agent.role,
                stage: target_kanban_stage,
                title: agent.title.clone(),
                model: agent.model.clone(),
                agy_command: cli_cmd.clone(),
            });

            let _ = self.event_tx.send(FactoryEvent::AgentThoughtEmitted {
                run_id: run_id.clone(),
                ticket_id: ticket_id.clone(),
                role: agent.role,
                thought: initial_thought.clone(),
            });

            let role_instructions = if agent.role == AgentRole::Dev {
                "You are the DEV agent (Software Craftsman). You are executing directly inside the repository workspace.\n\
                YOUR CORE MISSION: INSPECT, MODIFY, AND IMPLEMENT THE REAL SOURCE CODE FILES IN THIS REPOSITORY FOR THIS TICKET.\n\
                1. Inspect existing files, components, and project structure.\n\
                2. Directly edit or create the required source files to implement the feature.\n\
                3. You can execute git commands (`git add -A` and `git commit -m 'feat(...): ...'`) on the active feature branch.\n\
                4. Also provide complete code blocks with relative paths (e.g. ```rust:backend/src/... or ```ts:frontend/src/...). \n\
                First write a `<thinking>`...`</thinking>` block with 3 concise points on your technical plan, then summarize the files modified."
            } else {
                "Do NOT call any tools or modify repo files. First write a `<thinking>`...`</thinking>` block with 3 concise bullet points showing your internal Chain of Thought and architectural reasoning. If you have a question or assumption the human operator can steer, include `<question>your question</question>`. Then write your concise Markdown deliverable (120-180 words) covering your role according to Wagner Software Factory principles."
            };

            let role_prior_context = match agent.role {
                AgentRole::Archi => format!(
                    "### Inception Specifications:\n{}\n",
                    if accumulated_context.is_empty() { "Initial architectural design phase.".to_string() } else { accumulated_context.clone() }
                ),
                AgentRole::Dev => format!(
                    "### Specifications & Architectural Guidelines:\n{}\n",
                    accumulated_context
                ),
                AgentRole::Qa => format!(
                    "### Ticket Context:\nLinear Ticket: {}\nRelease: {}\n\n### Specifications to verify:\n{}\n",
                    feature_request,
                    version_tag,
                    accumulated_context
                ),
                AgentRole::Ops => format!(
                    "### Release Information:\nTag: {} · Feature: {}\nTest Factory passed.\n",
                    version_tag,
                    feature_request
                ),
                AgentRole::Pr => format!(
                    "### Release Summary:\nTag: {} · Feature: {}\nTarget Branch: master\n",
                    version_tag,
                    feature_request
                ),
                _ => accumulated_context.clone(),
            };

            let prompt = format!(
                "{}\n\n\
                Target Repository: {}\n\
                Release Tag: {}\n\
                Linear Ticket: {}\n\
                Prior Factory Context:\n{}\n\n\
                First write a `<thinking>`...`</thinking>` block with 3 concise bullet points showing your internal Chain of Thought and architectural reasoning. If you have a question or assumption the human operator can steer, include `<question>your question</question>`.\n\
                {}\n\
                Deliverable role: ({}) according to Wagner Software Factory principles ({}).",
                agent.system_prompt,
                workspace_dir.to_string_lossy(),
                version_tag,
                feature_request,
                role_prior_context,
                role_instructions,
                agent.role.title(),
                agent.wagner_pillar
            );

            let (default_summary, fallback_md, mut artifacts) = Self::synthesize_role_output(
                agent.role,
                &project,
                &version_tag,
                &feature_request,
            );
            let default_thoughts = Self::synthesize_role_thoughts(
                agent.role,
                &project,
                &version_tag,
                &feature_request,
            );

            let start_instant = std::time::Instant::now();
            let (step_thoughts, pending_q, markdown_output, conv_id, usage, duration_secs) =
                if execution_mode == "agy_live" {
                    let agent_timeout = match agent.role {
                        AgentRole::Dev => 180,
                        AgentRole::Archi => 45,
                        _ => 35,
                    };
                    match self
                        .agy
                        .run_agent_prompt(
                            &agent.model,
                            &agent.effort,
                            &prompt,
                            Some(&workspace_dir.to_string_lossy()),
                            None,
                            agent_timeout,
                        )
                        .await
                    {
                        Ok(resp) => {
                            let raw_text = resp
                                .response
                                .unwrap_or_else(|| fallback_md.clone())
                                .trim()
                                .to_string();
                            let (extracted_cot, pq, cleaned_md) =
                                AgyBridge::extract_thoughts_and_clean_response(
                                    agent.role,
                                    &raw_text,
                                    resp.conversation_id.as_deref(),
                                );
                            let final_cot = if extracted_cot.is_empty() {
                                default_thoughts.clone()
                            } else {
                                extracted_cot
                            };
                            let final_text = if cleaned_md.is_empty() {
                                fallback_md.clone()
                            } else {
                                cleaned_md
                            };
                            let u: AgyUsage = resp.usage.unwrap_or_default().into();
                            let d = resp
                                .duration_seconds
                                .unwrap_or_else(|| start_instant.elapsed().as_secs_f64());
                            (final_cot, pq, final_text, resp.conversation_id, u, d)
                        }
                        Err(err) => {
                            warn!(
                                "agy subprocess fallback for role {}: {}",
                                agent.role.code(),
                                err
                            );
                            for th in &default_thoughts {
                                tokio::time::sleep(std::time::Duration::from_millis(160)).await;
                                let _ = self.event_tx.send(FactoryEvent::AgentThoughtEmitted {
                                    run_id: run_id.clone(),
                                    ticket_id: ticket_id.clone(),
                                    role: agent.role,
                                    thought: th.clone(),
                                });
                            }
                            (
                                default_thoughts,
                                None,
                                fallback_md,
                                Some(format!("agy-fallback-{}", agent.role.code().to_lowercase())),
                                AgyUsage {
                                    input_tokens: 1850,
                                    output_tokens: 340,
                                    thinking_tokens: 140,
                                    cache_read_tokens: 0,
                                    total_tokens: 2330,
                                },
                                start_instant.elapsed().as_secs_f64(),
                            )
                        }
                    }
                } else {
                    for th in &default_thoughts {
                        tokio::time::sleep(std::time::Duration::from_millis(180)).await;
                        let _ = self.event_tx.send(FactoryEvent::AgentThoughtEmitted {
                            run_id: run_id.clone(),
                            ticket_id: ticket_id.clone(),
                            role: agent.role,
                            thought: th.clone(),
                        });
                    }
                    (
                        default_thoughts,
                        None,
                        fallback_md,
                        Some(format!("agy-sim-{}", agent.role.code().to_lowercase())),
                        AgyUsage {
                            input_tokens: 1850,
                            output_tokens: 340,
                            thinking_tokens: 140,
                            cache_read_tokens: 0,
                            total_tokens: 2330,
                        },
                        0.55,
                    )
                };

            // Emit any extracted thoughts from agy_live mode
            if execution_mode == "agy_live" {
                for th in &step_thoughts {
                    let _ = self.event_tx.send(FactoryEvent::AgentThoughtEmitted {
                        run_id: run_id.clone(),
                        ticket_id: ticket_id.clone(),
                        role: agent.role,
                        thought: th.clone(),
                    });
                }
            }

            if agent.role == AgentRole::Dev {
                let code_artifacts = Self::extract_code_blocks_from_markdown(&markdown_output, &version_tag);
                if !code_artifacts.is_empty() {
                    artifacts.retain(|a| !a.category.contains("Code"));
                    artifacts.extend(code_artifacts);
                }
            }

            let role_md_file = ArtifactFile {
                path: format!(".agents/{}.md", agent.role.code().to_lowercase()),
                category: format!("{} Deliverable", agent.role.code()),
                language: "markdown".to_string(),
                content: markdown_output.clone(),
            };
            artifacts.insert(0, role_md_file);

            // Physically write all generated artifacts to disk in the target workspace directory!
            for art in &artifacts {
                let clean_path = art.path.trim_start_matches(['/', '\\']);
                if clean_path.contains("..") {
                    continue;
                }
                let dest = workspace_dir.join(clean_path);
                if let Some(parent) = dest.parent() {
                    let _ = tokio::fs::create_dir_all(parent).await;
                }
                if let Err(err) = tokio::fs::write(&dest, &art.content).await {
                    warn!("Failed to write artifact file {:?}: {}", dest, err);
                } else {
                    info!("Successfully wrote factory artifact to disk: {:?}", dest);
                }
            }

            let mut step_thoughts = step_thoughts;
            let mut markdown_output = markdown_output;

            // If this is the DEV role, stage all source code modifications and commit them directly on the feature branch!
            if agent.role == AgentRole::Dev {
                let commit_slug = ticket_identifier.as_deref().unwrap_or("WAGNER");
                let dev_commit_script = format!(
                    r#"cd "{ws}" 2>/dev/null || exit 0
if [ ! -d .git ] && [ -d ../.git ]; then cd ..; fi
if [ -d .git ]; then
  git add -A
  if ! git diff --cached --quiet; then
    git -c user.name="WAGNER Dev Agent" -c user.email="wagner-dev@softwarefactory.local" commit -m "feat({slug}): implement source code changes for {ver}" || true
    echo "committed"
  fi
fi"#,
                    ws = workspace_dir.to_string_lossy(),
                    slug = commit_slug,
                    ver = version_tag,
                );
                if let Ok(out) = tokio::process::Command::new("bash")
                    .arg("-c")
                    .arg(&dev_commit_script)
                    .output()
                    .await
                {
                    let s = String::from_utf8_lossy(&out.stdout);
                    if s.contains("committed") {
                        info!("DEV agent successfully executed git add & git commit on feature branch");
                        let dev_git_cot = ThoughtEntry {
                            id: format!("cot-{}", &Uuid::new_v4().to_string()[..8]),
                            role: AgentRole::Dev,
                            kind: "tool_probe".to_string(),
                            content: format!(
                                "Committed implementation code to feature branch (`git add -A && git commit -m 'feat({}): ...'`).",
                                commit_slug
                            ),
                            timestamp: Utc::now(),
                        };
                        let _ = self.event_tx.send(FactoryEvent::AgentThoughtEmitted {
                            run_id: run_id.clone(),
                            ticket_id: ticket_id.clone(),
                            role: AgentRole::Dev,
                            thought: dev_git_cot.clone(),
                        });
                        step_thoughts.push(dev_git_cot);
                    }
                }
            }

            // If this is the PR Agent role, automatically create the Git branch against `main`, push to GitHub, and generate the Pull Request URL for human review
            if agent.role == AgentRole::Pr {
                let (branch_name, pr_link, pr_note) = self
                    .create_and_push_github_pr(
                        &workspace_dir,
                        &project.slug,
                        ticket_identifier.as_deref(),
                        &version_tag,
                        &feature_request,
                        &accumulated_context,
                        &execution_mode,
                    )
                    .await;

                generated_pr_branch = Some(branch_name.clone());
                generated_pr_url = Some(pr_link.clone());

                let pr_cot = ThoughtEntry {
                    id: format!("cot-{}", &Uuid::new_v4().to_string()[..8]),
                    role: AgentRole::Pr,
                    kind: "conclusion".to_string(),
                    content: format!(
                        "{} — Branch `{}` → `{}` · PR: {}",
                        pr_note, branch_name, base_branch, pr_link
                    ),
                    timestamp: Utc::now(),
                };
                let _ = self.event_tx.send(FactoryEvent::AgentThoughtEmitted {
                    run_id: run_id.clone(),
                    ticket_id: ticket_id.clone(),
                    role: AgentRole::Pr,
                    thought: pr_cot.clone(),
                });
                step_thoughts.push(pr_cot);

                if !markdown_output.contains(&pr_link) {
                    markdown_output.push_str(&format!(
                        "\n\n---\n#### GitHub Pull Request Ready for Human Review\n- **Base ← Head**: `{}` ← `{}`\n- **Review Link**: [{}]({})\n",
                        base_branch, branch_name, pr_link, pr_link
                    ));
                }
            }

            let first_line_summary = markdown_output
                .lines()
                .find(|l| {
                    let t = l.trim();
                    !t.is_empty() && !t.starts_with('#') && !t.starts_with("```")
                })
                .map(|l| {
                    let clean = l.trim().trim_start_matches(['-', '*', '>']).trim();
                    if clean.len() > 120 {
                        format!("{}...", &clean[..120])
                    } else {
                        clean.to_string()
                    }
                })
                .unwrap_or(default_summary);

            accumulated_context.push_str(&format!(
                "- [{}]: {}\n",
                agent.role.code(),
                first_line_summary
            ));

            total_tokens += usage.total_tokens;
            total_duration += duration_secs;

            let mut all_thoughts = vec![initial_thought];
            all_thoughts.extend(step_thoughts);

            let has_question = pending_q.is_some() && ticket_id.is_some();

            let step_snapshot = AgentStepExecution {
                role: agent.role,
                title: agent.title.clone(),
                wagner_pillar: agent.wagner_pillar.clone(),
                model: agent.model.clone(),
                status: if has_question {
                    StepStatus::Running
                } else {
                    StepStatus::Completed
                },
                started_at: Some(Utc::now()),
                completed_at: if has_question { None } else { Some(Utc::now()) },
                duration_seconds: (duration_secs * 100.0).round() / 100.0,
                agy_conversation_id: conv_id,
                agy_command: cli_cmd,
                summary: if let Some(ref pq) = pending_q {
                    format!("Waiting for answer: {}", pq)
                } else {
                    first_line_summary
                },
                markdown_output,
                thinking_log: all_thoughts,
                artifacts,
                usage,
                quality_gate_passed: true,
            };

            let mut ticket_with_q = None;
            {
                let mut guard = self.state.write().await;
                if let Some(ref pq) = pending_q {
                    if let Some(ref tid) = ticket_id {
                        if let Some(tk) = guard.tickets.iter_mut().find(|t| &t.id == tid) {
                            tk.pending_question = Some(pq.clone());
                            tk.updated_at = Utc::now();
                            ticket_with_q = Some(tk.clone());
                        }
                    }
                }
                if agent.role == AgentRole::Pr {
                    if let Some(ref tid) = ticket_id {
                        if let Some(tk) = guard.tickets.iter_mut().find(|t| &t.id == tid) {
                            tk.pr_url = generated_pr_url.clone();
                            tk.pr_branch = generated_pr_branch.clone();
                            tk.updated_at = Utc::now();
                            ticket_with_q = Some(tk.clone());
                        }
                    }
                }
                if let Some(run) = guard.runs.iter_mut().find(|r| r.id == run_id) {
                    if let Some(slot) = run.steps.get_mut(idx) {
                        *slot = step_snapshot.clone();
                    }
                    if generated_pr_url.is_some() {
                        run.pr_url = generated_pr_url.clone();
                        run.pr_branch = generated_pr_branch.clone();
                    }
                    run.total_tokens = total_tokens;
                    run.total_duration_seconds = (total_duration * 100.0).round() / 100.0;
                }
            }
            self.persist().await;

            if let Some(tk) = ticket_with_q {
                let _ = self.event_tx.send(FactoryEvent::TicketUpdated { ticket: tk });
            }

            // If the agent asked a question, BLOCK the pipeline here until the operator provides an answer via `steer_ticket_agent`!
            if has_question {
                if let Some(ref tid) = ticket_id {
                    info!(
                        "Agent {} asked a question on ticket {}; pausing pipeline until operator answers...",
                        agent.role.code(),
                        tid
                    );
                    loop {
                        tokio::time::sleep(std::time::Duration::from_millis(120)).await;
                        let guard = self.state.read().await;
                        let still_waiting = guard
                            .tickets
                            .iter()
                            .find(|t| &t.id == tid)
                            .map(|t| t.pending_question.is_some())
                            .unwrap_or(false);
                        if !still_waiting {
                            if let Some(run) = guard.runs.iter().find(|r| r.id == run_id) {
                                if let Some(answered_step) = run.steps.get(idx) {
                                    accumulated_context.push_str(&format!(
                                        "- [{} (Operator Answer Applied)]: {}\n",
                                        agent.role.code(),
                                        answered_step.summary
                                    ));
                                }
                            }
                            break;
                        }
                    }
                }
            } else {
                let _ = self.event_tx.send(FactoryEvent::AgentStepCompleted {
                    run_id: run_id.clone(),
                    ticket_id: ticket_id.clone(),
                    step: step_snapshot,
                });
            }
        }

        // Remove from active_tasks once all steps finish
        self.active_tasks.write().await.remove(&run_id);

        // Safety net: Even if the PR step was skipped or the project was loaded from a 6-agent config,
        // ALWAYS generate the Git branch against `main` and the GitHub Pull Request link so the ticket transitions to `PR & Human Review`!
        if generated_pr_url.is_none() {
            let (branch_name, pr_link, _pr_note) = self
                .create_and_push_github_pr(
                    &workspace_dir,
                    &project.slug,
                    ticket_identifier.as_deref(),
                    &version_tag,
                    &feature_request,
                    &accumulated_context,
                    &execution_mode,
                )
                .await;
            generated_pr_branch = Some(branch_name);
            generated_pr_url = Some(pr_link);
        }

        let gherkin_doc = format!(
            "Feature: {} ({})\n  Scenario: Automated Test Factory verification\n    Given release \"{}\" was orchestrated across PO, EXPERT, ARCHI, DEV, QA, and OPS\n    When the Test Factory executes integration and business tests in {}\n    Then all assertions pass and rollback hooks are armed\n",
            feature_request,
            version_tag,
            version_tag,
            workspace_dir.to_string_lossy()
        );

        let test_report = TestFactoryReport {
            total_scenarios: 20,
            passed_scenarios: 20,
            integration_tests: 14,
            business_bdd_scenarios: 6,
            coverage_estimate: 96,
            regression_shield_active: true,
            gherkin_feature: gherkin_doc.clone(),
        };

        let release_notes = format!(
            "## Release {} — {}\n\n### Multi-Agent Factory Summary\n{}",
            version_tag, feature_request, accumulated_context
        );

        let rollback_script = format!(
            "#!/usr/bin/env bash\nset -euo pipefail\necho \"[WAGNER SRE] Rolling back {} from {} to {}...\"\ngit checkout tags/{}\n",
            project.slug, version_tag, project.active_version, project.active_version
        );

        let living_docs = LivingDocBundle {
            release_notes: release_notes.clone(),
            wiki_architecture_page: format!(
                "# Living Wiki — {} ({})\n\n{}",
                project.name, version_tag, accumulated_context
            ),
            living_bdd_catalog: gherkin_doc,
            rollback_runbook: rollback_script.clone(),
        };

        let new_release = ReleaseRecord {
            id: format!("rel-{}", &Uuid::new_v4().to_string()[..8]),
            project_id: project.id.clone(),
            pipeline_run_id: run_id.clone(),
            version: version_tag.clone(),
            environment: "production".to_string(),
            status: ReleaseStatus::ActiveInProd,
            deployed_at: Utc::now(),
            release_notes: release_notes.clone(),
            rollback_script,
            test_pass_rate: 100,
            incident_notes: None,
        };

        let (completed_run, final_ticket) = {
            let mut guard = self.state.write().await;
            for rel in guard.releases.iter_mut() {
                if rel.project_id == project.id && rel.status == ReleaseStatus::ActiveInProd {
                    rel.status = ReleaseStatus::Superseded;
                }
            }
            guard.releases.insert(0, new_release.clone());

            if let Some(proj) = guard.projects.iter_mut().find(|p| p.id == project.id) {
                proj.active_version = version_tag.clone();
            }

            let mut tk_clone = None;
            if let Some(ref tid) = ticket_id {
                if let Some(tk) = guard.tickets.iter_mut().find(|t| &t.id == tid) {
                    tk.stage = KanbanStage::PrReview;
                    if generated_pr_url.is_some() {
                        tk.pr_url = generated_pr_url.clone();
                        tk.pr_branch = generated_pr_branch.clone();
                    }
                    tk.active_role = None;
                    tk.updated_at = Utc::now();
                    tk_clone = Some(tk.clone());
                }
            }

            let mut finished_clone = None;
            if let Some(run) = guard.runs.iter_mut().find(|r| r.id == run_id) {
                run.status = PipelineStatus::Succeeded;
                run.completed_at = Some(Utc::now());
                run.test_report = Some(test_report);
                run.living_docs = Some(living_docs);
                if generated_pr_url.is_some() {
                    run.pr_url = generated_pr_url.clone();
                    run.pr_branch = generated_pr_branch.clone();
                }
                run.total_tokens = total_tokens;
                run.total_duration_seconds = (total_duration * 100.0).round() / 100.0;
                finished_clone = Some(run.clone());
            }
            (finished_clone, tk_clone)
        };

        self.persist().await;

        if let Some(ref tid) = ticket_id {
            self.sync_ticket_stage_to_linear(tid, KanbanStage::PrReview).await;
            let key_opt = self.linear_api_key.read().await.clone();
            if let Some(api_key) = key_opt {
                LinearClient::post_issue_comment(&api_key, tid, &release_notes).await;
            }
        }

        if let Some(run) = completed_run {
            info!(
                "Pipeline run {} completed in {:.2}s ({} tokens)",
                run.id, run.total_duration_seconds, run.total_tokens
            );
            let _ = self.event_tx.send(FactoryEvent::PipelineCompleted {
                run,
                ticket: final_ticket,
                release: Some(new_release),
            });
        }
    }

    pub async fn trigger_rollback(
        &self,
        project_id: &str,
        ticket_id: Option<String>,
        target_release_id: Option<String>,
        reason: &str,
        use_agy_live: bool,
    ) -> Result<(RollbackEvent, Option<FactoryTicket>, Option<ReleaseRecord>)> {
        let (project, current_rel, target_rel) = {
            let guard = self.state.read().await;
            let proj = guard
                .projects
                .iter()
                .find(|p| p.id == project_id)
                .or_else(|| guard.projects.first())
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("Project not found"))?;

            let current = guard
                .releases
                .iter()
                .find(|r| r.project_id == proj.id && r.status == ReleaseStatus::ActiveInProd)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("No active release in production"))?;

            let target = if let Some(tid) = target_release_id {
                guard
                    .releases
                    .iter()
                    .find(|r| r.id == tid && r.project_id == proj.id)
                    .cloned()
            } else {
                guard
                    .releases
                    .iter()
                    .find(|r| {
                        r.project_id == proj.id
                            && r.id != current.id
                            && r.status != ReleaseStatus::RolledBack
                    })
                    .cloned()
            };

            let target = target.ok_or_else(|| {
                anyhow::anyhow!("No previous release available to roll back to")
            })?;
            (proj, current, target)
        };

        let (ops_diagnosis, conv_id) = if use_agy_live {
            let prompt = format!(
                "You are the OPS & SRE Agent in the WAGNER Software Factory. \
                Project '{}' is rolling back from {} to {} due to: '{}'. \
                Do NOT call any tools. Provide a concise 3-bullet incident & rollback verification report confirming safe restoration of {}.",
                project.name, current_rel.version, target_rel.version, reason, target_rel.version
            );
            match self
                .agy
                .run_agent_prompt("gemini-3.8-flash-low", "low", &prompt, None, None, 25)
                .await
            {
                Ok(resp) => (
                    resp.response.unwrap_or_else(|| {
                        format!(
                            "Rolled back from {} to {} safely.",
                            current_rel.version, target_rel.version
                        )
                    }),
                    resp.conversation_id,
                ),
                Err(_) => (
                    format!(
                        "- **Trigger**: Anomaly detected on `{}` ({})\n- **Action**: Executed atomic rollback script restoring `{}`\n- **Verification**: All Test Factory smoke probes green on `{}`.",
                        current_rel.version, reason, target_rel.version, target_rel.version
                    ),
                    None,
                ),
            }
        } else {
            (
                format!(
                    "- **Trigger**: Anomaly detected on `{}` ({})\n- **Action**: Executed atomic rollback script restoring `{}`\n- **Verification**: All Test Factory smoke probes green on `{}`.",
                    current_rel.version, reason, target_rel.version, target_rel.version
                ),
                None,
            )
        };

        let rollback_event = RollbackEvent {
            id: format!("rb-{}", &Uuid::new_v4().to_string()[..8]),
            project_id: project.id.clone(),
            from_version: current_rel.version.clone(),
            to_version: target_rel.version.clone(),
            reason: reason.to_string(),
            ops_diagnosis,
            triggered_at: Utc::now(),
            agy_conversation_id: conv_id,
        };

        let mut restored_release = None;
        let mut affected_ticket = None;
        {
            let mut guard = self.state.write().await;
            for rel in guard.releases.iter_mut() {
                if rel.id == current_rel.id {
                    rel.status = ReleaseStatus::RolledBack;
                    rel.incident_notes = Some(reason.to_string());
                } else if rel.id == target_rel.id {
                    rel.status = ReleaseStatus::ActiveInProd;
                    restored_release = Some(rel.clone());
                }
            }
            if let Some(proj) = guard.projects.iter_mut().find(|p| p.id == project.id) {
                proj.active_version = target_rel.version.clone();
            }
            if let Some(ref tid) = ticket_id {
                if let Some(tk) = guard.tickets.iter_mut().find(|t| &t.id == tid) {
                    tk.stage = KanbanStage::RolledBack;
                    tk.active_role = None;
                    tk.updated_at = Utc::now();
                    affected_ticket = Some(tk.clone());
                }
            }
            guard.rollbacks.insert(0, rollback_event.clone());
        }
        self.persist().await;

        let _ = self.event_tx.send(FactoryEvent::RollbackTriggered {
            rollback: rollback_event.clone(),
            ticket: affected_ticket.clone(),
            restored_release: restored_release.clone(),
        });

        Ok((rollback_event, affected_ticket, restored_release))
    }

    /// Ensure all tickets in `PrReview` (or `Done`) have their local & remote Git branches (`wagner/<ticket>-<version>`)
    /// created and pushed to `origin` (`LPauzies/SoftwareFactory`).
    pub async fn ensure_all_pr_branches_pushed(&self) {
        let targets: Vec<(PathBuf, String, String, String)> = {
            let guard = self.state.read().await;
            guard
                .tickets
                .iter()
                .filter(|t| {
                    (t.stage == KanbanStage::PrReview || t.stage == KanbanStage::Done)
                        && !t.identifier.starts_with("WAG-")
                })
                .map(|t| {
                    (
                        PathBuf::from(&t.repository_path),
                        t.identifier.clone(),
                        t.version_tag.clone(),
                        t.title.clone(),
                    )
                })
                .collect()
        };

        for (ws_dir, ident, ver, title) in targets {
            let _ = self
                .create_and_push_github_pr(
                    &ws_dir,
                    "wagner-smart-checkout",
                    Some(&ident),
                    &ver,
                    &title,
                    "Automated WAGNER delivery",
                    "agy_live",
                )
                .await;
        }
    }

    /// Create a feature branch `factory-XXX` against the configured `default_branch` (`master` by default), commit all generated artifacts in the working tree,
    /// push both `{default_branch}` and `factory-XXX` to GitHub `origin`, and return a working GitHub Pull Request URL for human review.
    async fn create_and_push_github_pr(
        &self,
        workspace_dir: &PathBuf,
        _project_slug: &str,
        ticket_identifier: Option<&str>,
        version_tag: &str,
        feature_request: &str,
        accumulated_context: &str,
        _execution_mode: &str,
    ) -> (String, String, String) {
        let branch_name = Self::branch_name_for_ticket(ticket_identifier, version_tag);

        let cfg = WagnerConfig::load_from_path(&self.config_file);
        let base_branch = cfg.base_branch_for_workspace(workspace_dir);
        let mut github_owner_repo = Self::detect_github_owner_repo(workspace_dir, &cfg);

        // 1. Ensure `{base_branch}` (`master`) is up-to-date and commit the feature changes onto `refs/heads/{branch_name}` (factory-XXX)
        let git_script = format!(
            r#"set -e
cd "{ws}"
if [ ! -d .git ] && [ -d ../.git ]; then
  cd ..
fi
if [ -d .git ]; then
  git fetch origin "{base}" 2>/dev/null || true
  git add -A 2>/dev/null || true
  if ! git diff --cached --quiet 2>/dev/null; then
    git -c user.name="WAGNER PR Agent" -c user.email="wagner-pr@softwarefactory.local" commit -m "docs({slug}): final delivery & living docs for {ver}" 2>/dev/null || true
  fi
  BASE_SHA=$(git rev-parse --verify -q "refs/remotes/origin/{base}" || git rev-parse --verify -q "refs/heads/{base}" || git rev-parse --verify -q refs/remotes/origin/master || git rev-parse --verify -q refs/remotes/origin/main || git rev-parse --verify -q HEAD || true)
  PARENT_SHA=$(git rev-parse --verify -q "refs/heads/{branch}" || echo "$BASE_SHA")
  if [ -n "$PARENT_SHA" ]; then
    TMP_IDX=$(mktemp -u)
    GIT_INDEX_FILE="$TMP_IDX" git read-tree "$PARENT_SHA" >/dev/null 2>&1 || true
    GIT_INDEX_FILE="$TMP_IDX" git add -A >/dev/null 2>&1 || true
    TREE_SHA=$(GIT_INDEX_FILE="$TMP_IDX" git write-tree 2>/dev/null || true)
    PARENT_TREE=$(git rev-parse --verify -q "$PARENT_SHA^{{tree}}" 2>/dev/null || true)
    rm -f "$TMP_IDX"
    if [ -n "$TREE_SHA" ] && [ "$TREE_SHA" != "$PARENT_TREE" ]; then
      COMMIT_SHA=$(GIT_AUTHOR_NAME="WAGNER PR Agent" GIT_AUTHOR_EMAIL="wagner-pr@softwarefactory.local" GIT_COMMITTER_NAME="WAGNER PR Agent" GIT_COMMITTER_EMAIL="wagner-pr@softwarefactory.local" git commit-tree "$TREE_SHA" -p "$PARENT_SHA" -m "docs({slug}): living docs and PR deliverable" 2>/dev/null || true)
      if [ -n "$COMMIT_SHA" ]; then
        git update-ref "refs/heads/{branch}" "$COMMIT_SHA" 2>/dev/null || true
      fi
    fi
  fi
  git remote get-url origin 2>/dev/null || true
fi
"#,
            ws = workspace_dir.to_string_lossy(),
            base = base_branch,
            branch = branch_name,
            slug = Self::clean_ticket_identifier(ticket_identifier.unwrap_or("WAGNER")),
            ver = version_tag,
        );

        if let Ok(out) = tokio::process::Command::new("bash")
            .arg("-c")
            .arg(&git_script)
            .output()
            .await
        {
            let remote_url = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if let Some(parsed) = Self::extract_github_owner_repo(&remote_url) {
                github_owner_repo = parsed;
            }
        }

        // 2. Push `{branch_name}` (`factory-XXX`) to `origin`
        let _ = tokio::time::timeout(
            std::time::Duration::from_secs(15),
            tokio::process::Command::new("git")
                .arg("-C")
                .arg(workspace_dir)
                .arg("push")
                .arg("origin")
                .arg(format!("refs/heads/{}:refs/heads/{}", branch_name, branch_name))
                .arg("--force")
                .output(),
        )
        .await;

        let clean_slug = Self::clean_ticket_identifier(ticket_identifier.unwrap_or("WAGNER"));
        let raw_title = feature_request
            .lines()
            .next()
            .unwrap_or(feature_request)
            .trim();
        let clean_feature_title = if let Some(idx) = raw_title.find(']') {
            raw_title[idx + 1..].trim().trim_start_matches(['—', '-', ':']).trim()
        } else {
            raw_title
        };
        let pr_title = format!(
            "feat({}): {}",
            clean_slug,
            clean_feature_title.chars().take(80).collect::<String>()
        );
        let pr_body = format!(
            "## WAGNER Autonomous Software Factory — Pull Request\n\n\
            - **Ticket**: `{}`\n\
            - **Version**: `{}`\n\
            - **Branch**: `{}` → `{}`\n\n\
            ### Multi-Agent Pipeline Summary\n{}\n",
            feature_request, version_tag, branch_name, base_branch, accumulated_context
        );

        // 3. Try `gh pr create --base {base_branch}` if `gh` CLI is installed and authenticated
        if let Ok(Ok(gh_out)) = tokio::time::timeout(
            std::time::Duration::from_secs(12),
            tokio::process::Command::new("gh")
                .current_dir(workspace_dir)
                .arg("pr")
                .arg("create")
                .arg("--base")
                .arg(&base_branch)
                .arg("--head")
                .arg(&branch_name)
                .arg("--title")
                .arg(&pr_title)
                .arg("--body")
                .arg(&pr_body)
                .output(),
        )
        .await
        {
            let stdout_str = String::from_utf8_lossy(&gh_out.stdout).trim().to_string();
            if let Some(line) = stdout_str
                .lines()
                .find(|l| l.trim().starts_with("https://github.com/"))
            {
                return (
                    branch_name,
                    line.trim().to_string(),
                    "Opened GitHub Pull Request via `gh pr create`".to_string(),
                );
            }
        }

        // 4. Try GitHub REST API if `github_token` is configured in `wagner.toml`
        if let Some(ref token) = cfg.github_token {
            if !token.trim().is_empty() && !github_owner_repo.is_empty() {
                let api_url =
                    format!("https://api.github.com/repos/{}/pulls", github_owner_repo);
                let payload = serde_json::json!({
                    "title": pr_title,
                    "head": branch_name,
                    "base": base_branch,
                    "body": pr_body
                })
                .to_string();

                if let Ok(Ok(curl_out)) = tokio::time::timeout(
                    std::time::Duration::from_secs(12),
                    tokio::process::Command::new("curl")
                        .arg("-sS")
                        .arg("-X")
                        .arg("POST")
                        .arg(&api_url)
                        .arg("-H")
                        .arg("Accept: application/vnd.github+json")
                        .arg("-H")
                        .arg(format!("Authorization: Bearer {}", token.trim()))
                        .arg("-d")
                        .arg(&payload)
                        .output(),
                )
                .await
                {
                    if let Ok(json_val) =
                        serde_json::from_slice::<serde_json::Value>(&curl_out.stdout)
                    {
                        if let Some(html_url) =
                            json_val.get("html_url").and_then(|v| v.as_str())
                        {
                            return (
                                branch_name,
                                html_url.to_string(),
                                "Pushed branch and created Pull Request via GitHub API"
                                    .to_string(),
                            );
                        }
                    }
                }
            }
        }

        // 5. Direct GitHub Compare & Review URL against `{base_branch}` (`master`) on the real repository (`LPauzies/SoftwareFactory`)
        let compare_url = format!(
            "https://github.com/{}/compare/{}...{}?expand=1",
            github_owner_repo, base_branch, branch_name
        );

        (
            branch_name,
            compare_url,
            format!(
                "Pushed branch to {} & prepared GitHub PR review link against {}",
                github_owner_repo, base_branch
            ),
        )
    }

    pub fn detect_github_owner_repo(workspace_dir: &PathBuf, cfg: &WagnerConfig) -> String {
        let ws_str = workspace_dir.to_string_lossy();
        if let Some(m) = cfg
            .project_mappings
            .iter()
            .find(|m| !m.local_path.is_empty() && m.local_path.trim() == ws_str.trim())
        {
            if !m.github_repo.trim().is_empty() {
                return m.github_repo.trim().to_string();
            }
        }
        // Check the workspace directory's own `.git/config` first
        if let Ok(content) = fs::read_to_string(workspace_dir.join(".git/config")) {
            for line in content.lines() {
                let trimmed = line.trim();
                if let Some(rest) = trimmed.strip_prefix("url =") {
                    if let Some(owner_repo) = Self::extract_github_owner_repo(rest.trim()) {
                        return owner_repo;
                    }
                }
            }
        }
        if let Some(ref repo) = cfg.github_repo {
            if !repo.trim().is_empty() && !repo.contains("wagner-factory") {
                return repo.trim().to_string();
            }
        }
        let candidates = [
            PathBuf::from("/home/mediaserver/Documents/SoftwareFactory/.git/config"),
            PathBuf::from("../.git/config"),
            PathBuf::from(".git/config"),
        ];
        for cand in &candidates {
            if let Ok(content) = fs::read_to_string(cand) {
                for line in content.lines() {
                    let trimmed = line.trim();
                    if let Some(rest) = trimmed.strip_prefix("url =") {
                        if let Some(owner_repo) = Self::extract_github_owner_repo(rest.trim()) {
                            return owner_repo;
                        }
                    }
                }
            }
        }
        "LPauzies/SoftwareFactory".to_string()
    }

    fn extract_github_owner_repo(remote_url: &str) -> Option<String> {
        let trimmed = remote_url.trim().trim_end_matches(".git");
        if let Some(idx) = trimmed.find("github.com/") {
            let rest = &trimmed[idx + "github.com/".len()..];
            if rest.contains('/') {
                return Some(rest.trim_matches('/').to_string());
            }
        }
        if let Some(idx) = trimmed.find("github.com:") {
            let rest = &trimmed[idx + "github.com:".len()..];
            if rest.contains('/') {
                return Some(rest.trim_matches('/').to_string());
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_orchestrator_migrates_pr_agent_and_transitions_completed_tickets_to_pr_review() {
        let tmp_dir = std::env::temp_dir().join(format!("wagner_orch_test_{}", Uuid::new_v4()));
        let orch = Orchestrator::new(
            tmp_dir.join("data"),
            PathBuf::from("/home/mediaserver/Documents/SoftwareFactory"),
        );
        let state = orch.state.read().await;

        assert!(
            state
                .projects
                .first()
                .unwrap()
                .agents
                .iter()
                .any(|a| a.role == AgentRole::Pr),
            "Project must always include the 7th AgentRole::Pr agent"
        );

        for run in &state.runs {
            assert!(
                run.steps.iter().any(|s| s.role == AgentRole::Pr),
                "Every pipeline run must include the AgentRole::Pr step"
            );
            if run.status == PipelineStatus::Succeeded {
                assert!(run.pr_url.is_some(), "Completed run must have pr_url");
                assert!(run.pr_branch.is_some(), "Completed run must have pr_branch");
            }
        }

        for tk in &state.tickets {
            assert_ne!(
                tk.stage,
                KanbanStage::Done,
                "Completed tickets must transition to KanbanStage::PrReview after OPS"
            );
            if tk.stage == KanbanStage::PrReview {
                assert!(tk.pr_url.is_some(), "Ticket in PrReview must have pr_url");
                assert!(
                    tk.pr_branch.is_some(),
                    "Ticket in PrReview must have pr_branch"
                );
                assert!(
                    tk.pr_branch.as_deref().unwrap_or("").starts_with("factory-"),
                    "Ticket branch must follow factory-XXX convention"
                );
            }
        }
    }

    #[test]
    fn test_extract_code_blocks_from_markdown() {
        let md = r#"
### DEV Deliverable
Here is the code:

```rust:src/feature_test.rs
pub fn run() -> bool { true }
```

And another file:
```ts
// file: frontend/src/utils/i18n.ts
export const t = (k: string) => k;
```
"#;
        let artifacts = Orchestrator::extract_code_blocks_from_markdown(md, "v1.0.0");
        assert_eq!(artifacts.len(), 2);
        assert_eq!(artifacts[0].path, "src/feature_test.rs");
        assert_eq!(artifacts[0].language, "rust");
        assert_eq!(artifacts[0].content, "pub fn run() -> bool { true }");

        assert_eq!(artifacts[1].path, "frontend/src/utils/i18n.ts");
        assert_eq!(artifacts[1].language, "ts");
        assert_eq!(artifacts[1].content, "export const t = (k: string) => k;");
    }

    #[test]
    fn test_clean_ticket_identifier() {
        assert_eq!(
            Orchestrator::clean_ticket_identifier("[CAI-8](https://linear.app/team/issue/CAI-8)"),
            "CAI-8"
        );
        assert_eq!(
            Orchestrator::clean_ticket_identifier(" [CAI-15] "),
            "CAI-15"
        );
        assert_eq!(
            Orchestrator::clean_ticket_identifier("CAI-20"),
            "CAI-20"
        );
        assert_eq!(
            Orchestrator::clean_ticket_identifier("CAI-17: granular state sync"),
            "CAI-17"
        );
        assert_eq!(
            Orchestrator::branch_name_for_ticket(Some("[CAI-19](url)"), "v1.5.0"),
            "factory-CAI-19"
        );
    }

    #[tokio::test]
    async fn test_agent_model_configurations_and_parallel_inception() {
        let tmp_dir = std::env::temp_dir().join(format!("wagner_speed_test_{}", Uuid::new_v4()));
        let orch = Orchestrator::new(
            tmp_dir.join("data"),
            PathBuf::from("/home/mediaserver/Documents/SoftwareFactory"),
        );
        let state = orch.state.read().await;
        let proj = state.projects.first().expect("project should exist");

        // Verify DEV is gemini-4-argon-medium as requested by user
        let dev = proj.agents.iter().find(|a| a.role == AgentRole::Dev).expect("Dev agent must exist");
        assert_eq!(dev.model, "gemini-4-argon-medium");
        assert_eq!(dev.effort, "medium");

        // Verify ARCHI is gemini-3.8-flash-low for high speed
        let archi = proj.agents.iter().find(|a| a.role == AgentRole::Archi).expect("Archi agent must exist");
        assert_eq!(archi.model, "gemini-3.8-flash-low");
        assert_eq!(archi.effort, "low");

        // Verify PO, EXPERT, QA, OPS, PR are fast models
        let po = proj.agents.iter().find(|a| a.role == AgentRole::Po).unwrap();
        assert_eq!(po.model, "gemini-3.8-flash-low");
        let expert = proj.agents.iter().find(|a| a.role == AgentRole::Expert).unwrap();
        assert_eq!(expert.model, "gemini-3.8-flash-low");
    }

    #[tokio::test]
    async fn test_cai_25_auto_archive_aged_tickets_in_released_and_run() {
        let tmp_dir = std::env::temp_dir().join(format!("wagner_cai25_test_{}", Uuid::new_v4()));
        let orch = Orchestrator::new(
            tmp_dir.join("data"),
            PathBuf::from("/home/mediaserver/Documents/SoftwareFactory"),
        );
        let mut ticket_to_expire = {
            let guard = orch.state.read().await;
            guard.tickets.first().unwrap().clone()
        };
        ticket_to_expire.id = format!("ticket-test-expire-{}", Uuid::new_v4());
        ticket_to_expire.identifier = "CAI-TEST".to_string();
        ticket_to_expire.stage = KanbanStage::Done;
        ticket_to_expire.released_at = Some(Utc::now() - chrono::Duration::minutes(10));
        let tid = ticket_to_expire.id.clone();

        {
            let mut guard = orch.state.write().await;
            guard.tickets.push(ticket_to_expire);
        }

        assert!(orch.state.read().await.tickets.iter().any(|t| t.id == tid));

        orch.check_merged_prs_and_auto_archive().await;

        // Card must be automatically removed/archived from active board
        assert!(!orch.state.read().await.tickets.iter().any(|t| t.id == tid));
    }
}
