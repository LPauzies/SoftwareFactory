use crate::models::{FactoryTicket, KanbanStage, LinearProject, LinearStatus};
use anyhow::{anyhow, Result};
use chrono::Utc;
use serde_json::Value;
use std::process::Stdio;
use tokio::process::Command;
use tracing::{info, warn};

pub struct LinearClient;

impl LinearClient {
    /// Sync projects, teams, and active (non-deleted/non-archived) issues from Linear's GraphQL API (`https://api.linear.app/graphql`).
    pub async fn sync_from_linear(
        api_key: &str,
        default_workspace_root: &str,
    ) -> Result<(LinearStatus, Vec<LinearProject>, Vec<FactoryTicket>)> {
        let trimmed_key = api_key.trim();
        if trimmed_key.is_empty() {
            return Err(anyhow!("Linear API key is empty"));
        }

        let query = r#"
        query WagnerFactorySync {
          viewer {
            name
          }
          organization {
            name
          }
          projects(first: 50) {
            nodes {
              id
              name
              description
              archivedAt
              trashed
            }
          }
          teams(first: 20) {
            nodes {
              id
              key
              name
              description
            }
          }
          issues(first: 100) {
            nodes {
              id
              identifier
              title
              description
              priorityLabel
              url
              trashed
              archivedAt
              canceledAt
              createdAt
              updatedAt
              state {
                name
                type
              }
              project {
                id
                name
              }
              team {
                id
                key
                name
              }
            }
          }
        }
        "#;

        let body = serde_json::json!({ "query": query }).to_string();

        let output = Command::new("curl")
            .arg("-sS")
            .arg("-X")
            .arg("POST")
            .arg("https://api.linear.app/graphql")
            .arg("-H")
            .arg("Content-Type: application/json")
            .arg("-H")
            .arg("Cache-Control: no-cache")
            .arg("-H")
            .arg(format!("Authorization: {}", trimmed_key))
            .arg("-d")
            .arg(&body)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await?;

        if !output.status.success() {
            let err_str = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!("curl to Linear GraphQL failed: {}", err_str));
        }

        let resp_json: Value = serde_json::from_slice(&output.stdout)
            .map_err(|e| anyhow!("Invalid JSON from Linear API: {}", e))?;

        if let Some(errors) = resp_json.get("errors") {
            return Err(anyhow!("Linear GraphQL error: {}", errors));
        }

        let data = resp_json
            .get("data")
            .ok_or_else(|| anyhow!("Missing 'data' field in Linear response"))?;

        let viewer_name = data
            .pointer("/viewer/name")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let org_name = data
            .pointer("/organization/name")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let mut projects = Vec::new();

        // Parse active Linear projects first
        if let Some(nodes) = data.pointer("/projects/nodes").and_then(|v| v.as_array()) {
            for node in nodes {
                let trashed = node
                    .get("trashed")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let archived = node.get("archivedAt").is_some_and(|v| !v.is_null());
                if trashed || archived {
                    continue;
                }

                let id = node.get("id").and_then(|v| v.as_str()).unwrap_or_default();
                let name = node
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Linear Project");
                let desc = node
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Synced from Linear");
                if !id.is_empty() {
                    let key: String = name
                        .chars()
                        .filter(|c| c.is_alphabetic())
                        .take(4)
                        .collect::<String>()
                        .to_uppercase();
                    projects.push(LinearProject {
                        id: id.to_string(),
                        name: name.to_string(),
                        key: if key.is_empty() {
                            "LIN".to_string()
                        } else {
                            key
                        },
                        description: desc.to_string(),
                        default_repo_path: default_workspace_root.to_string(),
                        default_github_repo: String::new(),
                        source: "linear_api".to_string(),
                    });
                }
            }
        }

        // Only include Linear teams as fallback if the Linear workspace has zero Projects
        if projects.is_empty() {
            if let Some(nodes) = data.pointer("/teams/nodes").and_then(|v| v.as_array()) {
                for node in nodes {
                    let id = node.get("id").and_then(|v| v.as_str()).unwrap_or_default();
                    let name = node
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("Linear Team");
                    let key = node.get("key").and_then(|v| v.as_str()).unwrap_or("TEAM");
                    let desc = node
                        .get("description")
                        .and_then(|v| v.as_str())
                        .unwrap_or("Linear Team Workspace");
                    if !id.is_empty() && !projects.iter().any(|p| p.id == id) {
                        projects.push(LinearProject {
                            id: id.to_string(),
                            name: name.to_string(),
                            key: key.to_string(),
                            description: desc.to_string(),
                            default_repo_path: default_workspace_root.to_string(),
                            default_github_repo: String::new(),
                            source: "linear_api".to_string(),
                        });
                    }
                }
            }
        }

        let mut tickets = Vec::new();
        if let Some(nodes) = data.pointer("/issues/nodes").and_then(|v| v.as_array()) {
            for (idx, node) in nodes.iter().enumerate() {
                // Skip any issue that was deleted (trashed), archived, or canceled in Linear
                let trashed = node
                    .get("trashed")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let archived = node.get("archivedAt").is_some_and(|v| !v.is_null());
                let canceled = node.get("canceledAt").is_some_and(|v| !v.is_null());
                let state_type = node
                    .pointer("/state/type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unstarted");

                if trashed || archived || canceled || state_type == "canceled" {
                    continue;
                }

                let id = node.get("id").and_then(|v| v.as_str()).unwrap_or_default();
                if id.is_empty() {
                    continue;
                }
                let identifier = node
                    .get("identifier")
                    .and_then(|v| v.as_str())
                    .unwrap_or("LIN-1");
                let title = node
                    .get("title")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Untitled Issue");
                let description = node
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("No description provided in Linear.");
                let priority_label = node
                    .get("priorityLabel")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Medium");
                let url = node
                    .get("url")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());

                let team_id = node
                    .pointer("/team/id")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());

                let proj_id = node
                    .pointer("/project/id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unassigned");

                let stage = match state_type {
                    "completed" => KanbanStage::Done,
                    "started" => KanbanStage::Coding,
                    _ => KanbanStage::Todo,
                };

                let created_at = node
                    .get("createdAt")
                    .and_then(|v| v.as_str())
                    .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(Utc::now);

                let updated_at = node
                    .get("updatedAt")
                    .and_then(|v| v.as_str())
                    .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(Utc::now);

                tickets.push(FactoryTicket {
                    id: id.to_string(),
                    identifier: identifier.to_string(),
                    title: title.to_string(),
                    description: description.to_string(),
                    priority_label: priority_label.to_string(),
                    linear_project_id: proj_id.to_string(),
                    linear_team_id: team_id,
                    linear_url: url,
                    repository_path: default_workspace_root.to_string(),
                    github_repo: String::new(),
                    stage,
                    active_role: None,
                    active_run_id: None,
                    pending_question: None,
                    pr_url: None,
                    pr_branch: None,
                    version_tag: format!("v1.{}.0", idx + 1),
                    created_at,
                    updated_at,
                });
            }
        }

        info!(
            "Synced {} projects and {} active issues from Linear org {:?}",
            projects.len(),
            tickets.len(),
            org_name
        );

        let status = LinearStatus {
            connected: true,
            has_api_key: true,
            organization_name: org_name,
            viewer_name,
            last_synced_at: Some(Utc::now()),
        };

        Ok((status, projects, tickets))
    }

    /// Post a progress or completion comment on a real Linear issue when an API key is configured.
    pub async fn post_issue_comment(api_key: &str, issue_id: &str, body_md: &str) {
        let trimmed = api_key.trim();
        if trimmed.is_empty() || issue_id.starts_with("ticket-") {
            return;
        }
        let mutation = r#"
        mutation CommentCreate($issueId: String!, $body: String!) {
          commentCreate(input: { issueId: $issueId, body: $body }) {
            success
          }
        }
        "#;
        let payload = serde_json::json!({
            "query": mutation,
            "variables": {
                "issueId": issue_id,
                "body": body_md
            }
        })
        .to_string();

        if let Err(e) = Command::new("curl")
            .arg("-sS")
            .arg("-X")
            .arg("POST")
            .arg("https://api.linear.app/graphql")
            .arg("-H")
            .arg("Content-Type: application/json")
            .arg("-H")
            .arg(format!("Authorization: {}", trimmed))
            .arg("-d")
            .arg(&payload)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await
        {
            warn!("Failed to post Linear comment: {}", e);
        }
    }

    /// Delete (trash) an issue in Linear when deleted from a Wagner Kanban column.
    pub async fn delete_issue(api_key: &str, issue_id: &str) {
        let trimmed = api_key.trim();
        if trimmed.is_empty() || issue_id.starts_with("ticket-") {
            return;
        }
        let mutation = r#"
        mutation IssueDelete($id: String!) {
          issueDelete(id: $id) {
            success
          }
        }
        "#;
        let payload = serde_json::json!({
            "query": mutation,
            "variables": {
                "id": issue_id
            }
        })
        .to_string();

        let _ = Command::new("curl")
            .arg("-sS")
            .arg("-X")
            .arg("POST")
            .arg("https://api.linear.app/graphql")
            .arg("-H")
            .arg("Content-Type: application/json")
            .arg("-H")
            .arg(format!("Authorization: {}", trimmed))
            .arg("-d")
            .arg(&payload)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await;
    }
}
