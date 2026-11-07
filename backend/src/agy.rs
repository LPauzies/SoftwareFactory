use crate::models::{AgentRole, AgyModelInfo, AgyStatus, AgyUsage, ThoughtEntry};
use anyhow::{anyhow, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::path::Path;
use std::collections::HashMap;
use std::process::Stdio;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use tokio::process::Command;
use tokio::time::timeout;
use tracing::{info, warn};
use uuid::Uuid;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AgyPrintResponse {
    pub conversation_id: Option<String>,
    pub status: Option<String>,
    pub response: Option<String>,
    pub duration_seconds: Option<f64>,
    pub num_turns: Option<u32>,
    pub usage: Option<AgyRawUsage>,
    /// Set by agy when the turn failed (e.g. `RESOURCE_EXHAUSTED` quota error).
    #[serde(default)]
    pub error: Option<String>,
    /// Model that actually served the turn (may differ from the requested one after a fallback).
    #[serde(skip)]
    pub model_used: Option<String>,
}

/// Model families by preference: best first. Quota-exhausted families fall back to the next one.
const MODEL_PREFERENCE: [&str; 3] = ["gemini-4-argon", "claude-opus-5-5", "gemini-3.8-flash"];
/// How long a model that hit its quota is skipped before being retried.
const EXHAUSTED_TTL: Duration = Duration::from_secs(3600);

fn exhausted_models() -> &'static Mutex<HashMap<String, Instant>> {
    static CELL: OnceLock<Mutex<HashMap<String, Instant>>> = OnceLock::new();
    CELL.get_or_init(|| Mutex::new(HashMap::new()))
}

fn is_exhausted(model: &str) -> bool {
    exhausted_models()
        .lock()
        .map(|m| m.get(model).is_some_and(|t| t.elapsed() < EXHAUSTED_TTL))
        .unwrap_or(false)
}

fn mark_exhausted(model: &str) {
    if let Ok(mut m) = exhausted_models().lock() {
        m.insert(model.to_string(), Instant::now());
    }
}

/// Errors meaning "this model is not usable on the subscription right now" (worth trying the next one).
fn is_availability_error(err: &str) -> bool {
    let e = err.to_lowercase();
    [
        "quota",
        "resource_exhausted",
        "429",
        "not available",
        "upgrade your subscription",
        "content safety",
        "safety filter",
        "blocked by content",
        "blocked by safety",
        "rate limit",
        "overloaded",
    ]
    .iter()
    .any(|k| e.contains(k))
}

/// Candidate models for `requested`: itself, then the lower-preference families with the same effort suffix.
/// e.g. `gemini-4-argon-medium` -> [argon-medium, claude-opus-5-5-medium, gemini-3.8-flash-medium].
/// A model outside the preference list (e.g. sonnet) is tried first, then the whole list.
pub fn fallback_chain(requested: &str) -> Vec<String> {
    let pos = MODEL_PREFERENCE
        .iter()
        .position(|f| requested.starts_with(&format!("{}-", f)));
    let suffix = match pos {
        Some(i) => requested[MODEL_PREFERENCE[i].len() + 1..].to_string(),
        None => requested.rsplit('-').next().unwrap_or("medium").to_string(),
    };
    let suffix = if ["low", "medium", "high"].contains(&suffix.as_str()) { suffix } else { "medium".to_string() };
    let mut chain = vec![requested.to_string()];
    for f in &MODEL_PREFERENCE[pos.map_or(0, |i| i + 1)..] {
        let m = format!("{}-{}", f, suffix);
        if !chain.contains(&m) {
            chain.push(m);
        }
    }
    chain
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct AgyRawUsage {
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
    #[serde(default)]
    pub thinking_tokens: u64,
    #[serde(default)]
    pub cache_read_tokens: u64,
    #[serde(default)]
    pub total_tokens: u64,
}

impl From<AgyRawUsage> for AgyUsage {
    fn from(raw: AgyRawUsage) -> Self {
        Self {
            input_tokens: raw.input_tokens,
            output_tokens: raw.output_tokens,
            thinking_tokens: raw.thinking_tokens,
            cache_read_tokens: raw.cache_read_tokens,
            total_tokens: raw.total_tokens,
        }
    }
}

#[derive(Debug, Clone)]
pub struct AgyBridge {
    pub binary_path: String,
}

impl AgyBridge {
    pub fn new() -> Self {
        if let Ok(env_bin) = std::env::var("AGY_BIN") {
            if !env_bin.trim().is_empty() {
                return Self {
                    binary_path: env_bin,
                };
            }
        }
        let candidates = [
            "/usr/local/bin/agy",
            "/home/mediaserver/.local/bin/agy",
            "/usr/bin/agy",
            "agy",
        ];
        let mut chosen = "agy".to_string();
        for c in candidates {
            if c != "agy" && Path::new(c).exists() {
                chosen = c.to_string();
                break;
            }
        }
        Self {
            binary_path: chosen,
        }
    }

    pub fn default_gemini_models() -> Vec<AgyModelInfo> {
        vec![
            AgyModelInfo {
                id: "gemini-3.8-flash-low".to_string(),
                label: "Gemini 3.8 Flash (Low) — Fast Factory Agent".to_string(),
                family: "gemini-3.8-flash".to_string(),
            },
            AgyModelInfo {
                id: "gemini-3.8-flash-medium".to_string(),
                label: "Gemini 3.8 Flash (Medium) — Balanced Factory Agent".to_string(),
                family: "gemini-3.8-flash".to_string(),
            },
            AgyModelInfo {
                id: "gemini-3.8-flash-high".to_string(),
                label: "Gemini 3.8 Flash (High) — Deep Reasoning Flash".to_string(),
                family: "gemini-3.8-flash".to_string(),
            },
            AgyModelInfo {
                id: "gemini-4-argon-low".to_string(),
                label: "Gemini 4 Argon (Low) — Flagship Low Effort".to_string(),
                family: "gemini-4-argon".to_string(),
            },
            AgyModelInfo {
                id: "gemini-4-argon-medium".to_string(),
                label: "Gemini 4 Argon (Medium) — Flagship Architect/Dev".to_string(),
                family: "gemini-4-argon".to_string(),
            },
            AgyModelInfo {
                id: "gemini-4-argon-high".to_string(),
                label: "Gemini 4 Argon (High) — Deep Flagship Reasoning".to_string(),
                family: "gemini-4-argon".to_string(),
            },
        ]
    }

    pub async fn check_status(&self) -> AgyStatus {
        let available = Path::new(&self.binary_path).exists()
            || Command::new(&self.binary_path)
                .arg("--help")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .await
                .map(|s| s.success())
                .unwrap_or(false);

        let models = self.fetch_models().await;
        let (brain_sessions_count, brain_steps_count) = Self::scan_local_brain_activity();
        AgyStatus {
            binary_path: self.binary_path.clone(),
            available,
            version_hint: "Antigravity CLI (agy --mode=auto --print --output-format json)"
                .to_string(),
            models,
            brain_sessions_count,
            brain_steps_count,
        }
    }

    fn scan_local_brain_activity() -> (usize, usize) {
        let brain_dir = Path::new("/home/mediaserver/.gemini/antigravity-cli/brain");
        let mut sessions = 0usize;
        let mut steps = 0usize;
        if let Ok(entries) = fs::read_dir(brain_dir) {
            for entry in entries.flatten() {
                if entry.path().is_dir() {
                    sessions += 1;
                    let transcript = entry
                        .path()
                        .join(".system_generated/logs/transcript.jsonl");
                    if let Ok(content) = fs::read_to_string(transcript) {
                        steps += content.lines().count();
                    }
                }
            }
        }
        (sessions, steps)
    }

    pub async fn fetch_models(&self) -> Vec<AgyModelInfo> {
        let cmd_fut = Command::new(&self.binary_path)
            .arg("models")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output();

        if let Ok(Ok(output)) = timeout(Duration::from_secs(6), cmd_fut).await {
            if output.status.success() {
                let text = String::from_utf8_lossy(&output.stdout);
                let mut parsed = Vec::new();
                for line in text.lines() {
                    let clean = line.trim();
                    if clean.is_empty() || clean.contains("Fetching available models") {
                        continue;
                    }
                    let clean = clean.rsplit('\r').next().unwrap_or(clean).trim();
                    let mut parts = clean.splitn(2, char::is_whitespace);
                    if let (Some(id), Some(label)) = (parts.next(), parts.next()) {
                        let id = id.trim();
                        let label = label.trim();
                        if id.starts_with("gemini-") {
                            let family = if id.starts_with("gemini-4") {
                                "gemini-4-argon"
                            } else {
                                "gemini-3.8-flash"
                            };
                            parsed.push(AgyModelInfo {
                                id: id.to_string(),
                                label: label.to_string(),
                                family: family.to_string(),
                            });
                        }
                    }
                }
                if !parsed.is_empty() {
                    return parsed;
                }
            }
        }

        Self::default_gemini_models()
    }

    pub fn format_cli_command(&self, model: &str, effort: &str, role_code: &str) -> String {
        format!(
            "{} --mode=auto --print \"[Role: {}] ...\" --model {} --effort {} --disable-slash-commands --output-format json",
            self.binary_path, role_code, model, effort
        )
    }

    /// Run a turn on the best available model: `model` first, then the next families of
    /// `MODEL_PREFERENCE` (Argon > Opus > Flash) when the subscription quota is exhausted.
    pub async fn run_agent_prompt(
        &self,
        model: &str,
        effort: &str,
        prompt: &str,
        workspace_dir: Option<&str>,
        conversation_id: Option<&str>,
    ) -> Result<AgyPrintResponse> {
        let chain = fallback_chain(model);
        let mut last_err = None;
        for candidate in &chain {
            if is_exhausted(candidate) && candidate != chain.last().unwrap_or(candidate) {
                info!("Skipping model {} (quota exhausted recently)", candidate);
                continue;
            }
            match self
                .run_single_prompt(candidate, effort, prompt, workspace_dir, conversation_id)
                .await
            {
                Ok(mut resp) => {
                    if candidate != model {
                        warn!("Model {} unavailable, turn served by fallback model {}", model, candidate);
                    }
                    resp.model_used = Some(candidate.clone());
                    return Ok(resp);
                }
                Err(e) if is_availability_error(&e.to_string()) => {
                    warn!("Model {} unavailable on subscription: {}", candidate, e);
                    mark_exhausted(candidate);
                    last_err = Some(e);
                }
                Err(e) => return Err(e),
            }
        }
        Err(last_err.unwrap_or_else(|| anyhow!("no model available among {:?}", chain)))
    }

    /// Execute a single non-interactive turn against Antigravity (`agy --mode=auto`) via subprocess.
    /// Runs until completion with full auto-approved permissions and no artificial timeout.
    async fn run_single_prompt(
        &self,
        model: &str,
        effort: &str,
        prompt: &str,
        workspace_dir: Option<&str>,
        conversation_id: Option<&str>,
    ) -> Result<AgyPrintResponse> {
        info!(
            "Spawning agy subprocess (--mode=auto --dangerously-skip-permissions): binary={}, model={}, effort={}",
            self.binary_path, model, effort
        );

        let mut cmd = Command::new(&self.binary_path);
        cmd.kill_on_drop(true)
            .arg("--mode=auto")
            .arg("--dangerously-skip-permissions")
            .arg("--print-timeout")
            .arg("0")
            .arg("--print")
            .arg(prompt)
            .arg("--model")
            .arg(model)
            .arg("--effort")
            .arg(effort)
            .arg("--disable-slash-commands")
            .arg("--output-format")
            .arg("json")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        if let Some(dir) = workspace_dir {
            if Path::new(dir).exists() {
                cmd.arg("--add-dir").arg(dir);
                cmd.current_dir(dir);
            }
        }

        if let Some(conv_id) = conversation_id {
            if !conv_id.is_empty() && !conv_id.starts_with("agy-sim-") && !conv_id.starts_with("agy-fallback-") {
                cmd.arg("--conversation").arg(conv_id);
            }
        }

        let output = match cmd.output().await {
            Ok(out) => out,
            Err(e) => return Err(anyhow!("Failed to spawn agy subprocess: {}", e)),
        };

        let stdout_str = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr_str = String::from_utf8_lossy(&output.stderr).to_string();

        // agy reports turn failures (quota, auth, model unavailable...) as `"status":"ERROR"` + `"error"`.
        for line in stdout_str.lines().rev() {
            if let Ok(parsed) = serde_json::from_str::<AgyPrintResponse>(line.trim()) {
                if let Some(err) = parsed.error.as_deref().filter(|e| !e.trim().is_empty()) {
                    return Err(anyhow!("agy error (model {}): {}", model, err.trim()));
                }
                break;
            }
        }

        for line in stdout_str.lines().rev() {
            let trimmed = line.trim();
            if trimmed.starts_with('{') && trimmed.ends_with('}') {
                if let Ok(parsed) = serde_json::from_str::<AgyPrintResponse>(trimmed) {
                    if parsed.response.is_some() {
                        return Ok(parsed);
                    }
                }
            }
        }

        if let Ok(parsed) = serde_json::from_str::<AgyPrintResponse>(stdout_str.trim()) {
            return Ok(parsed);
        }

        if output.status.success() && !stdout_str.trim().is_empty() {
            return Ok(AgyPrintResponse {
                conversation_id: None,
                status: Some("SUCCESS".to_string()),
                response: Some(stdout_str),
                duration_seconds: Some(1.5),
                num_turns: Some(1),
                usage: Some(AgyRawUsage::default()),
                error: None,
                model_used: None,
            });
        }

        warn!(
            "agy subprocess returned non-JSON or error. status={:?}, stderr={}",
            output.status, stderr_str
        );
        Err(anyhow!(
            "agy exited with status {:?}: {}",
            output.status.code(),
            stderr_str
        ))
    }

    /// Extract Chain-of-Thought (`<thinking>...</thinking>`), optional `<question>...</question>`,
    /// and any native `thinking` field from `~/.gemini/antigravity-cli/brain/<conv_id>/.system_generated/logs/transcript.jsonl`.
    pub fn extract_thoughts_and_clean_response(
        role: AgentRole,
        raw_response: &str,
        conversation_id: Option<&str>,
    ) -> (Vec<ThoughtEntry>, Option<String>, String) {
        let mut thoughts = Vec::new();
        let mut pending_question: Option<String> = None;
        let mut cleaned = raw_response.to_string();

        // 1. Check Antigravity transcript.jsonl if conversation_id is available
        if let Some(conv_id) = conversation_id {
            let transcript_path = format!(
                "/home/mediaserver/.gemini/antigravity-cli/brain/{}/.system_generated/logs/transcript.jsonl",
                conv_id
            );
            if let Ok(content) = fs::read_to_string(&transcript_path) {
                for line in content.lines() {
                    if let Ok(val) = serde_json::from_str::<Value>(line) {
                        if let Some(thinking_str) = val.get("thinking").and_then(|v| v.as_str()) {
                            for bullet in thinking_str.lines() {
                                let b = bullet.trim();
                                if !b.is_empty() {
                                    thoughts.push(ThoughtEntry {
                                        id: format!("cot-{}", &Uuid::new_v4().to_string()[..8]),
                                        role,
                                        kind: "reasoning".to_string(),
                                        content: b.to_string(),
                                        timestamp: Utc::now(),
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }

        // 2. Parse explicit <thinking>...</thinking> block from response
        if let (Some(start), Some(end)) = (cleaned.find("<thinking>"), cleaned.find("</thinking>"))
        {
            if start < end {
                let inside = &cleaned[start + 10..end];
                for line in inside.lines() {
                    let trimmed = line.trim().trim_start_matches(['-', '*', '•']).trim();
                    if !trimmed.is_empty() {
                        thoughts.push(ThoughtEntry {
                            id: format!("cot-{}", &Uuid::new_v4().to_string()[..8]),
                            role,
                            kind: "reasoning".to_string(),
                            content: trimmed.to_string(),
                            timestamp: Utc::now(),
                        });
                    }
                }
                let mut next_clean = String::new();
                next_clean.push_str(&cleaned[..start]);
                next_clean.push_str(&cleaned[end + 11..]);
                cleaned = next_clean.trim().to_string();
            }
        }

        // 3. Parse optional <question>...</question> block if agent has a question for the user
        if let (Some(q_start), Some(q_end)) =
            (cleaned.find("<question>"), cleaned.find("</question>"))
        {
            if q_start < q_end {
                let q_text = cleaned[q_start + 10..q_end].trim().to_string();
                if !q_text.is_empty() && !q_text.eq_ignore_ascii_case("none") {
                    pending_question = Some(q_text.clone());
                    thoughts.push(ThoughtEntry {
                        id: format!("cot-{}", &Uuid::new_v4().to_string()[..8]),
                        role,
                        kind: "question".to_string(),
                        content: q_text,
                        timestamp: Utc::now(),
                    });
                }
                let mut next_clean = String::new();
                next_clean.push_str(&cleaned[..q_start]);
                next_clean.push_str(&cleaned[q_end + 11..]);
                cleaned = next_clean.trim().to_string();
            }
        }

        (thoughts, pending_question, cleaned)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fallback_chain_follows_preference_order() {
        assert_eq!(
            fallback_chain("gemini-4-argon-medium"),
            vec!["gemini-4-argon-medium", "claude-opus-5-5-medium", "gemini-3.8-flash-medium"]
        );
        assert_eq!(fallback_chain("claude-opus-5-5-low"), vec!["claude-opus-5-5-low", "gemini-3.8-flash-low"]);
        assert_eq!(fallback_chain("gemini-3.8-flash-high"), vec!["gemini-3.8-flash-high"]);
        assert_eq!(
            fallback_chain("claude-sonnet-5-5-medium"),
            vec!["claude-sonnet-5-5-medium", "gemini-4-argon-medium", "claude-opus-5-5-medium", "gemini-3.8-flash-medium"]
        );
    }

    #[test]
    fn detects_quota_errors() {
        assert!(is_availability_error("agy error: Individual quota reached. Please upgrade your subscription"));
        assert!(is_availability_error("Your previous response was blocked by content safety filters Output retries exhausted after 4 attempts"));
        assert!(!is_availability_error("Failed to spawn agy subprocess: No such file"));
    }
}
