# DevSecFinOps & SRE Deliverable — Release v1.3.0 (`[CAI-18]`)

### 1. CI/CD & DevSecOps Automation
- **Pipeline Stages**: Lint, Unit/Gherkin Tests, SAST (`semgrep`), and Container Image Build.
- **Release Automation**: Automatic generation of `RELEASE_NOTES.md` and release tagging for `v1.3.0` upon successful merge to the default branch.

### 2. RUN Observability & Monitoring
- **Metrics**: 
  - `pr_agent_target_branch_resolutions_total{status="success|failure"}`
  - `pr_agent_open_duration_seconds` (p95/p99 latency)
  - `pr_agent_upstream_rate_limits_remaining`
- **SLO & Alerting**: Target branch resolution success rate > 99.5%; alert on consecutive fallback invocations or elevated 4xx/5xx API responses.

### 3. Explicit Rollback Strategy
- **Rollback Script (`scripts/rollback-v1.3.0.sh`)**:
  ```bash
  #!/usr/bin/env bash
  set -euo pipefail
  echo "Rolling back to v1.2.x stable release..."
  git checkout tags/v1.2.9 -b hotfix-rollback-v1.3.0
  # Toggle feature-flag disablement for dynamic remote branch resolution
  export PR_AGENT_DYNAMIC_TARGET_RESOLVER_ENABLED=false
  deploy --target=production --ref=v1.2.9
  ```