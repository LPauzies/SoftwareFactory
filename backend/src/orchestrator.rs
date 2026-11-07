/// Deterministic PR title formatter (`[CAI-XX] <Sujet métier tronqué à 72 caractères>`) without branch-slug or `#15 #15` noise (CAI-26).
    pub fn format_pr_title(ticket_identifier: Option<&str>, feature_or_title: &str) -> String {
        let resolved_id = ticket_identifier
            .map(Self::clean_ticket_identifier)
            .filter(|s| !s.is_empty() && s != "WAGNER")
            .unwrap_or_else(|| Self::clean_ticket_identifier(feature_or_title));

        let mut s = feature_or_title
            .lines()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("")
            .trim();

        while s.starts_with('[') {
            if let Some(end) = s.find(']') {
                s = s[end + 1..].trim();
                if s.starts_with('(') {
                    if let Some(p_end) = s.find(')') {
                        s = s[p_end + 1..].trim();
                    }
                }
            } else {
                break;
            }
        }

        if s.len() >= resolved_id.len() && s[..resolved_id.len()].eq_ignore_ascii_case(&resolved_id) {
            s = s[resolved_id.len()..].trim();
        }
        s = s.trim_start_matches(['—', '–', '-', ':']).trim();

        if let Some((before_dash, _)) = s.split_once(" — ") {
            s = before_dash.trim();
        }

        loop {
            let trimmed = s.trim_end_matches(['—', '–', '-', ':', ' ']);
            if let Some(hash_idx) = trimmed.rfind('#') {
                let after_hash = &trimmed[hash_idx + 1..];
                if !after_hash.is_empty() && after_hash.chars().all(|c| c.is_ascii_digit()) {
                    let before_hash = &trimmed[..hash_idx];
                    if before_hash.is_empty()
                        || before_hash.ends_with(' ')
                        || before_hash.ends_with('-')
                    {
                        s = before_hash.trim_end_matches(['—', '–', '-', ':', ' ']);
                        continue;
                    }
                }
            }
            s = trimmed;
            break;
        }

        let lower = s.to_lowercase();
        let is_branch_placeholder = lower.starts_with("factory ")
            || lower.starts_with("factory-")
            || lower == resolved_id.to_lowercase();

        let subject: String = if s.is_empty() || is_branch_placeholder {
            "Automated Factory PR".to_string()
        } else {
            s.chars().take(72).collect::<String>().trim_end().to_string()
        };

        format!("[{}] {}", resolved_id, subject)
    }

    /// Percent-encode a query parameter value for GitHub compare URLs.
    /// shortcut: inline RFC 3986 byte encoding without extra crate dependency, upgrade to url::form_urlencoded if multi-param builders are needed.
    pub fn url_encode_query_value(val: &str) -> String {
        let mut encoded = String::with_capacity(val.len() * 2);
        for b in val.as_bytes() {
            match *b {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                    encoded.push(*b as char);
                }
                _ => {
                    encoded.push_str(&format!("%{:02X}", b));
                }
            }
        }
        encoded
    }

    /// Build a GitHub compare & PR creation URL that pre-fills the PR title (`&title=...`) on click (CAI-26).
    pub fn build_github_compare_url(
        github_repo: &str,
        base_branch: &str,
        head_branch: &str,
        pr_title: &str,
    ) -> String {
        format!(
            "https://github.com/{}/compare/{}...{}?expand=1&title={}",
            github_repo,
            base_branch,
            head_branch,
            Self::url_encode_query_value(pr_title)
        )
    }