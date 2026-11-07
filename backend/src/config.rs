use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// 1:1:1 Binding between a Linear Project, a Local Git Repository path, and a GitHub Repository (`owner/repo`).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectRepoMapping {
    pub linear_project: String,
    pub local_path: String,
    pub github_repo: String,
    #[serde(default = "default_branch_name")]
    pub default_branch: String,
}

fn default_branch_name() -> String {
    "master".to_string()
}

#[derive(Debug, Clone, Default)]
pub struct WagnerConfig {
    pub config_path: PathBuf,
    pub linear_api_key: Option<String>,
    pub github_token: Option<String>,
    pub github_repo: Option<String>,
    pub gitlab_token: Option<String>,
    pub gemini_api_key: Option<String>,
    pub default_repository_path: Option<String>,
    pub default_branch: String,
    pub agy_binary_path: Option<String>,
    pub linear_sync_interval_seconds: u64,
    pub project_mappings: Vec<ProjectRepoMapping>,
    pub extra_secrets: HashMap<String, String>,
}

impl WagnerConfig {
    /// Locate `wagner.toml` in the workspace root (or `WAGNER_CONFIG` env var).
    pub fn resolve_path() -> PathBuf {
        if let Ok(custom) = std::env::var("WAGNER_CONFIG") {
            let p = PathBuf::from(custom.trim());
            if !p.as_os_str().is_empty() {
                return p;
            }
        }
        let candidates = [
            PathBuf::from("wagner.toml"),
            PathBuf::from("../wagner.toml"),
            PathBuf::from("/app/wagner.toml"),
        ];
        for c in &candidates {
            if c.exists() {
                return c.clone();
            }
        }
        // If running inside `backend/`, prefer `../wagner.toml` if parent has `justfile` or `frontend`
        if Path::new("../frontend").exists() || Path::new("../justfile").exists() {
            return PathBuf::from("../wagner.toml");
        }
        PathBuf::from("wagner.toml")
    }

    /// Load configuration & secrets from `wagner.toml` (falling back to environment variables if set).
    pub fn load() -> Self {
        let path = Self::resolve_path();
        Self::load_from_path(&path)
    }

    pub fn load_from_path(path: &Path) -> Self {
        let mut cfg = Self {
            config_path: path.to_path_buf(),
            default_branch: "master".to_string(),
            linear_sync_interval_seconds: 20,
            ..Default::default()
        };

        if let Ok(raw) = fs::read_to_string(path) {
            let mut current_section = String::new();
            let mut current_project: Option<ProjectRepoMapping> = None;

            for line in raw.lines() {
                let trimmed = Self::strip_inline_comment(line).trim();
                if trimmed.is_empty() || trimmed.starts_with('#') {
                    continue;
                }
                if trimmed.starts_with("[[") && trimmed.ends_with("]]") && trimmed.len() >= 4 {
                    if let Some(mut proj) = current_project.take() {
                        if proj.default_branch.is_empty() {
                            proj.default_branch = cfg.default_branch.clone();
                        }
                        if !proj.linear_project.is_empty() {
                            cfg.project_mappings.push(proj);
                        }
                    }
                    current_section = trimmed[2..trimmed.len() - 2].trim().to_lowercase();
                    if current_section == "projects" || current_section == "project" {
                        current_project = Some(ProjectRepoMapping {
                            default_branch: cfg.default_branch.clone(),
                            ..Default::default()
                        });
                    }
                    continue;
                }
                if trimmed.starts_with('[') && trimmed.ends_with(']') {
                    if let Some(mut proj) = current_project.take() {
                        if proj.default_branch.is_empty() {
                            proj.default_branch = cfg.default_branch.clone();
                        }
                        if !proj.linear_project.is_empty() {
                            cfg.project_mappings.push(proj);
                        }
                    }
                    current_section = trimmed[1..trimmed.len() - 1].trim().to_lowercase();
                    continue;
                }
                if let Some((k, v)) = trimmed.split_once('=') {
                    let key = k.trim().to_lowercase();
                    let val = Self::unquote(v.trim());
                    if val.is_empty() {
                        continue;
                    }
                    if let Some(ref mut proj) = current_project {
                        match key.as_str() {
                            "linear_project" | "name" | "project" => proj.linear_project = val,
                            "local_path" | "repository_path" | "local_repo" | "path" => {
                                proj.local_path = val
                            }
                            "github_repo" | "github" | "remote_repo" => proj.github_repo = val,
                            "default_branch" | "base_branch" | "branch" => {
                                proj.default_branch = val
                            }
                            _ => {}
                        }
                        continue;
                    }
                    match (current_section.as_str(), key.as_str()) {
                        (_, "linear_api_key") => cfg.linear_api_key = Some(val),
                        (_, "github_token") => cfg.github_token = Some(val),
                        (_, "github_repo") => cfg.github_repo = Some(val),
                        (_, "gitlab_token") => cfg.gitlab_token = Some(val),
                        (_, "gemini_api_key") => cfg.gemini_api_key = Some(val),
                        (_, "default_repository_path") => cfg.default_repository_path = Some(val),
                        (_, "default_branch" | "base_branch") => cfg.default_branch = val,
                        (_, "agy_binary_path") => cfg.agy_binary_path = Some(val),
                        (_, "linear_sync_interval_seconds") => {
                            if let Ok(secs) = val.parse::<u64>() {
                                if secs >= 5 {
                                    cfg.linear_sync_interval_seconds = secs;
                                }
                            }
                        }
                        ("secrets", other) => {
                            cfg.extra_secrets.insert(other.to_string(), val);
                        }
                        _ => {}
                    }
                }
            }
            if let Some(mut proj) = current_project.take() {
                if proj.default_branch.is_empty() {
                    proj.default_branch = cfg.default_branch.clone();
                }
                if !proj.linear_project.is_empty() {
                    cfg.project_mappings.push(proj);
                }
            }
        }

        // Derive default_repository_path and github_repo from the first [[projects]] entry if not explicitly set
        if cfg.default_repository_path.is_none() {
            cfg.default_repository_path = cfg
                .project_mappings
                .iter()
                .find(|m| !m.local_path.trim().is_empty())
                .map(|m| m.local_path.trim().to_string());
        }
        if cfg.github_repo.is_none() {
            cfg.github_repo = cfg
                .project_mappings
                .iter()
                .find(|m| !m.github_repo.trim().is_empty())
                .map(|m| m.github_repo.trim().to_string());
        }

        // Environment variable overrides
        if let Ok(env_lin) = std::env::var("LINEAR_API_KEY") {
            let t = env_lin.trim();
            if !t.is_empty() {
                cfg.linear_api_key = Some(t.to_string());
            }
        }
        if let Ok(env_gem) = std::env::var("GEMINI_API_KEY") {
            let t = env_gem.trim();
            if !t.is_empty() {
                cfg.gemini_api_key = Some(t.to_string());
            }
        }
        if let Ok(env_gh) = std::env::var("GITHUB_TOKEN") {
            let t = env_gh.trim();
            if !t.is_empty() {
                cfg.github_token = Some(t.to_string());
            }
        }

        cfg
    }

    /// Persist `linear_api_key` directly into `wagner.toml` while preserving all comments and other keys.
    pub fn save_linear_api_key(path: &Path, new_key: &str) {
        let trimmed_key = new_key.trim();
        if trimmed_key.is_empty() {
            return;
        }
        if let Ok(existing) = fs::read_to_string(path) {
            let mut updated_lines = Vec::new();
            let mut replaced = false;
            for line in existing.lines() {
                let code_part = Self::strip_inline_comment(line).trim();
                if let Some((k, _)) = code_part.split_once('=') {
                    if k.trim().eq_ignore_ascii_case("linear_api_key") {
                        updated_lines.push(format!("linear_api_key = \"{}\"", trimmed_key));
                        replaced = true;
                        continue;
                    }
                }
                updated_lines.push(line.to_string());
            }
            if !replaced {
                updated_lines.push(String::new());
                updated_lines.push("[secrets]".to_string());
                updated_lines.push(format!("linear_api_key = \"{}\"", trimmed_key));
            }
            let _ = fs::write(path, updated_lines.join("\n") + "\n");
        } else {
            let default_toml = format!(
                "# WAGNER — Configuration & Secrets (wagner.toml)\n\n[secrets]\nlinear_api_key = \"{}\"\ngithub_token = \"\"\ngitlab_token = \"\"\ngemini_api_key = \"\"\n",
                trimmed_key
            );
            let _ = fs::write(path, default_toml);
        }
    }

    /// Find a configured `[[projects]]` mapping matching a Linear project's name, key, or ID.
    pub fn find_mapping_for_project(&self, name: &str, key: &str, id: &str) -> Option<&ProjectRepoMapping> {
        let name_low = name.trim().to_lowercase();
        let key_low = key.trim().to_lowercase();
        let id_low = id.trim().to_lowercase();
        self.project_mappings.iter().find(|m| {
            let target = m.linear_project.trim().to_lowercase();
            !target.is_empty()
                && (target == name_low
                    || target == key_low
                    || target == id_low
                    || name_low.contains(&target)
                    || target.contains(&name_low))
        })
    }

    /// Resolve the configured base branch (`default_branch`, defaulting to `"master"`) for a workspace directory.
    pub fn base_branch_for_workspace(&self, workspace_dir: &Path) -> String {
        let ws = workspace_dir.to_string_lossy();
        if let Some(m) = self
            .project_mappings
            .iter()
            .find(|m| !m.local_path.trim().is_empty() && m.local_path.trim() == ws.trim())
        {
            if !m.default_branch.trim().is_empty() {
                return m.default_branch.trim().to_string();
            }
        }
        if !self.default_branch.trim().is_empty() {
            self.default_branch.trim().to_string()
        } else {
            "master".to_string()
        }
    }

    /// Upsert a `[[projects]]` block (`linear_project`, `local_path`, `github_repo`) in `wagner.toml`.
    pub fn upsert_project_mapping(path: &Path, mapping: &ProjectRepoMapping) {
        if mapping.linear_project.trim().is_empty() {
            return;
        }
        let mut cfg = Self::load_from_path(path);
        let target_low = mapping.linear_project.trim().to_lowercase();
        if let Some(existing) = cfg
            .project_mappings
            .iter_mut()
            .find(|m| m.linear_project.trim().to_lowercase() == target_low)
        {
            if !mapping.local_path.trim().is_empty() {
                existing.local_path = mapping.local_path.trim().to_string();
            }
            if !mapping.github_repo.trim().is_empty() {
                existing.github_repo = mapping.github_repo.trim().to_string();
            }
        } else {
            cfg.project_mappings.push(mapping.clone());
        }

        // Rewrite the non-`[[projects]]` portion of `wagner.toml` and append all `[[projects]]` blocks cleanly
        let existing_raw = fs::read_to_string(path).unwrap_or_default();
        let mut kept_lines = Vec::new();
        let mut in_projects_block = false;
        for line in existing_raw.lines() {
            let trimmed = Self::strip_inline_comment(line).trim();
            if trimmed.starts_with("[[") && trimmed.ends_with("]]") && trimmed.len() >= 4 {
                let sec = trimmed[2..trimmed.len() - 2].trim().to_lowercase();
                in_projects_block = sec == "projects" || sec == "project";
                if in_projects_block {
                    continue;
                }
            } else if trimmed.starts_with('[') && trimmed.ends_with(']') {
                in_projects_block = false;
            }
            if !in_projects_block {
                kept_lines.push(line.to_string());
            }
        }
        while kept_lines.last().is_some_and(|l| l.trim().is_empty()) {
            kept_lines.pop();
        }
        for m in &cfg.project_mappings {
            kept_lines.push(String::new());
            kept_lines.push("[[projects]]".to_string());
            kept_lines.push(format!("linear_project = \"{}\"", m.linear_project));
            kept_lines.push(format!("local_path = \"{}\"", m.local_path));
            kept_lines.push(format!("github_repo = \"{}\"", m.github_repo));
        }
        let _ = fs::write(path, kept_lines.join("\n") + "\n");
    }

    fn strip_inline_comment(line: &str) -> &str {
        let mut in_single = false;
        let mut in_double = false;
        let mut prev_backslash = false;
        for (idx, ch) in line.char_indices() {
            match ch {
                '"' if !in_single && !prev_backslash => in_double = !in_double,
                '\'' if !in_double && !prev_backslash => in_single = !in_single,
                '#' if !in_single && !in_double => return &line[..idx],
                _ => {}
            }
            prev_backslash = ch == '\\' && !prev_backslash;
        }
        line
    }

    fn unquote(val: &str) -> String {
        if (val.starts_with('"') && val.ends_with('"') && val.len() >= 2)
            || (val.starts_with('\'') && val.ends_with('\'') && val.len() >= 2)
        {
            val[1..val.len() - 1].trim().to_string()
        } else {
            val.trim().to_string()
        }
    }
}
