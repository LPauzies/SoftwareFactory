use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// The 7 multi-disciplinary roles orchestrated inside the WAGNER Autonomous Software Factory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AgentRole {
    Po,
    Expert,
    Archi,
    Dev,
    Qa,
    Ops,
    Pr,
}

impl AgentRole {
    #[allow(dead_code)]
    pub fn all() -> Vec<AgentRole> {
        vec![
            AgentRole::Po,
            AgentRole::Expert,
            AgentRole::Archi,
            AgentRole::Dev,
            AgentRole::Qa,
            AgentRole::Ops,
            AgentRole::Pr,
        ]
    }

    pub fn code(&self) -> &'static str {
        match self {
            AgentRole::Po => "PO",
            AgentRole::Expert => "EXPERT",
            AgentRole::Archi => "ARCHI",
            AgentRole::Dev => "DEV",
            AgentRole::Qa => "QA",
            AgentRole::Ops => "OPS",
            AgentRole::Pr => "PR",
        }
    }

    pub fn title(&self) -> &'static str {
        match self {
            AgentRole::Po => "Product Owner (PO)",
            AgentRole::Expert => "Business Expert (Expert Métier)",
            AgentRole::Archi => "Software Architect (ARCHI)",
            AgentRole::Dev => "Software Crafts(wo)man (DEV)",
            AgentRole::Qa => "Test Factory Engineer (QA)",
            AgentRole::Ops => "DevSecOps & SRE (OPS)",
            AgentRole::Pr => "GitHub PR & Release Integrator (PR)",
        }
    }

    pub fn wagner_pillar(&self) -> &'static str {
        match self {
            AgentRole::Po => "Gagner en productivité & Orientation Utilisateur",
            AgentRole::Expert => "Consolider la documentation vivante & Règles Métier",
            AgentRole::Archi => "Homogénéité de l'Usine Logicielle & Modularité",
            AgentRole::Dev => "Sécuriser le code source & Software Craftsmanship",
            AgentRole::Qa => "Améliorer la qualité (Test Factory raccordée)",
            AgentRole::Ops => "Accélérer les déploiements, Améliorer le RUN & Rollback",
            AgentRole::Pr => "Génération automatique de la PR contre main & Revue Humaine",
        }
    }

    pub fn corresponding_brick(&self) -> BrickKind {
        match self {
            AgentRole::Po => BrickKind::ProjectManagement,
            AgentRole::Expert => BrickKind::LivingDocumentation,
            AgentRole::Archi => BrickKind::ScmSecurity,
            AgentRole::Dev => BrickKind::ScmSecurity,
            AgentRole::Qa => BrickKind::TestFactory,
            AgentRole::Ops => BrickKind::ContinuousDeployment,
            AgentRole::Pr => BrickKind::ScmSecurity,
        }
    }

    pub fn kanban_stage(&self) -> KanbanStage {
        match self {
            AgentRole::Po | AgentRole::Expert | AgentRole::Archi => KanbanStage::Design,
            AgentRole::Dev => KanbanStage::Coding,
            AgentRole::Qa => KanbanStage::Testing,
            AgentRole::Ops => KanbanStage::Testing,
            AgentRole::Pr => KanbanStage::PrReview,
        }
    }
}

/// Simplified Kanban stages representing the WAGNER Software Factory lifecycle
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KanbanStage {
    Todo,
    Design,
    Coding,
    Testing,
    Done,
    PrReview,
    RolledBack,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinearProject {
    pub id: String,
    pub name: String,
    pub key: String,
    pub description: String,
    pub default_repo_path: String,
    #[serde(default)]
    pub default_github_repo: String,
    pub source: String, // "linear_api" | "demo"
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FactoryTicket {
    pub id: String,
    pub identifier: String,
    pub title: String,
    pub description: String,
    pub priority_label: String,
    pub linear_project_id: String,
    #[serde(default)]
    pub linear_team_id: Option<String>,
    pub linear_url: Option<String>,
    pub repository_path: String,
    #[serde(default)]
    pub github_repo: String,
    pub stage: KanbanStage,
    pub active_role: Option<AgentRole>,
    pub active_run_id: Option<String>,
    #[serde(default)]
    pub pending_question: Option<String>,
    #[serde(default)]
    pub pr_url: Option<String>,
    #[serde(default)]
    pub pr_branch: Option<String>,
    pub version_tag: String,
    #[serde(default = "Utc::now")]
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(default)]
    pub released_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub archived: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LinearWorkflowState {
    pub id: String,
    pub name: String,
    pub state_type: String, // "backlog", "unstarted", "started", "completed", "canceled"
    pub team_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LinearStatus {
    pub connected: bool,
    pub has_api_key: bool,
    pub organization_name: Option<String>,
    pub viewer_name: Option<String>,
    pub last_synced_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub workflow_states: Vec<LinearWorkflowState>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrickKind {
    ProjectManagement,
    ScmSecurity,
    TestFactory,
    LivingDocumentation,
    ContinuousDeployment,
    RunAndRollback,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FactoryBrick {
    pub kind: BrickKind,
    pub name: String,
    pub french_subtitle: String,
    pub description: String,
    pub enabled: bool,
    pub provider_plugin: String,
    pub health_score: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketplacePlugin {
    pub id: String,
    pub name: String,
    pub category: String,
    pub brick_kind: BrickKind,
    pub description: String,
    pub installed: bool,
    pub compatible_with: Vec<String>,
    pub icon: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    pub role: AgentRole,
    pub title: String,
    pub wagner_pillar: String,
    pub model: String,
    pub effort: String,
    pub enabled: bool,
    pub system_prompt: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AgyUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub thinking_tokens: u64,
    pub cache_read_tokens: u64,
    pub total_tokens: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepStatus {
    Pending,
    Running,
    Completed,
    Skipped,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactFile {
    pub path: String,
    pub category: String,
    pub language: String,
    pub content: String,
}

/// A single entry in the agent's real-time Chain of Thought (CoT) & Human Steering log
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThoughtEntry {
    pub id: String,
    pub role: AgentRole,
    /// "reasoning" | "tool_probe" | "question" | "user_steering" | "conclusion"
    pub kind: String,
    pub content: String,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentStepExecution {
    pub role: AgentRole,
    pub title: String,
    pub wagner_pillar: String,
    pub model: String,
    pub status: StepStatus,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub duration_seconds: f64,
    pub agy_conversation_id: Option<String>,
    pub agy_command: String,
    pub summary: String,
    pub markdown_output: String,
    #[serde(default)]
    pub thinking_log: Vec<ThoughtEntry>,
    pub artifacts: Vec<ArtifactFile>,
    pub usage: AgyUsage,
    pub quality_gate_passed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PipelineStatus {
    Queued,
    Running,
    Succeeded,
    Failed,
    RolledBack,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestFactoryReport {
    pub total_scenarios: u32,
    pub passed_scenarios: u32,
    pub integration_tests: u32,
    pub business_bdd_scenarios: u32,
    pub coverage_estimate: u8,
    pub regression_shield_active: bool,
    pub gherkin_feature: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LivingDocBundle {
    pub release_notes: String,
    pub wiki_architecture_page: String,
    pub living_bdd_catalog: String,
    pub rollback_runbook: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineRun {
    pub id: String,
    pub project_id: String,
    #[serde(default)]
    pub ticket_id: Option<String>,
    #[serde(default)]
    pub ticket_identifier: Option<String>,
    pub version_tag: String,
    pub feature_request: String,
    pub execution_mode: String, // "agy_live" | "agy_fast_simulation"
    pub status: PipelineStatus,
    pub created_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub steps: Vec<AgentStepExecution>,
    pub test_report: Option<TestFactoryReport>,
    pub living_docs: Option<LivingDocBundle>,
    #[serde(default)]
    pub pr_url: Option<String>,
    #[serde(default)]
    pub pr_branch: Option<String>,
    pub total_tokens: u64,
    pub total_duration_seconds: f64,
    pub workspace_dir: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReleaseStatus {
    ActiveInProd,
    StagingReady,
    Superseded,
    RolledBack,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReleaseRecord {
    pub id: String,
    pub project_id: String,
    pub pipeline_run_id: String,
    pub version: String,
    pub environment: String,
    pub status: ReleaseStatus,
    pub deployed_at: DateTime<Utc>,
    pub release_notes: String,
    pub rollback_script: String,
    pub test_pass_rate: u8,
    pub incident_notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RollbackEvent {
    pub id: String,
    pub project_id: String,
    pub from_version: String,
    pub to_version: String,
    pub reason: String,
    pub ops_diagnosis: String,
    pub triggered_at: DateTime<Utc>,
    pub agy_conversation_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub slug: String,
    pub description: String,
    pub domain: String,
    pub repository_path: String,
    pub active_version: String,
    pub created_at: DateTime<Utc>,
    pub bricks: Vec<FactoryBrick>,
    pub agents: Vec<AgentConfig>,
    pub homogeneity_score: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FactoryState {
    pub projects: Vec<Project>,
    #[serde(default)]
    pub linear_projects: Vec<LinearProject>,
    #[serde(default)]
    pub tickets: Vec<FactoryTicket>,
    #[serde(default)]
    pub dismissed_ticket_ids: Vec<String>,
    #[serde(default)]
    pub linear_status: LinearStatus,
    pub marketplace: Vec<MarketplacePlugin>,
    pub runs: Vec<PipelineRun>,
    pub releases: Vec<ReleaseRecord>,
    pub rollbacks: Vec<RollbackEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgyModelInfo {
    pub id: String,
    pub label: String,
    pub family: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgyStatus {
    pub binary_path: String,
    pub available: bool,
    pub version_hint: String,
    pub models: Vec<AgyModelInfo>,
    #[serde(default)]
    pub brain_sessions_count: usize,
    #[serde(default)]
    pub brain_steps_count: usize,
}

/// Real-time Server-Sent Events emitted during WAGNER multi-agent orchestration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event_type", rename_all = "snake_case")]
pub enum FactoryEvent {
    TicketUpdated {
        ticket: FactoryTicket,
    },
    TicketDeleted {
        ticket_id: String,
        identifier: String,
    },
    PipelineStarted {
        run_id: String,
        project_id: String,
        ticket_id: Option<String>,
        version_tag: String,
        feature_request: String,
    },
    AgentStepStarted {
        run_id: String,
        ticket_id: Option<String>,
        role: AgentRole,
        stage: KanbanStage,
        title: String,
        model: String,
        agy_command: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        started_at: Option<DateTime<Utc>>,
    },
    AgentThoughtEmitted {
        run_id: String,
        ticket_id: Option<String>,
        role: AgentRole,
        thought: ThoughtEntry,
    },
    AgentStepCompleted {
        run_id: String,
        ticket_id: Option<String>,
        step: AgentStepExecution,
    },
    PipelineCompleted {
        run: PipelineRun,
        ticket: Option<FactoryTicket>,
        release: Option<ReleaseRecord>,
    },
    RollbackTriggered {
        rollback: RollbackEvent,
        ticket: Option<FactoryTicket>,
        restored_release: Option<ReleaseRecord>,
    },
    AgentCancelled {
        run_id: Option<String>,
        ticket: FactoryTicket,
        killed_role: Option<AgentRole>,
    },
    LinearSynced {
        linear_status: LinearStatus,
        linear_projects: Vec<LinearProject>,
        tickets: Vec<FactoryTicket>,
    },
}
