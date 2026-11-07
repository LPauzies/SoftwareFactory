### Product Owner Specification: [CAI-18] Automatic PR Creation

#### Objective
Enable the Autonomous PR Agent to automatically target and open Pull Requests against the repository's default branch, accelerating delivery cycles and eliminating manual context switching for developers.

#### User Story
**As a** software engineer using the WAGNER platform,  
**I want** completed agent work to automatically create a Pull Request against the default branch,  
**So that** my code changes enter the review pipeline instantly with zero manual overhead.

#### Acceptance Criteria
1. **Dynamic Target Discovery**: Resolves the target repository's active default branch (e.g., `main`/`master`) dynamically via Git/GitHub API.
2. **Contextual Metadata**: PR title and structured description (summary, related ticket `[CAI-18]`, test status) are populated automatically.
3. **Idempotency & Safety**: Detects if an open PR already exists for the head branch to prevent duplicate submissions.
4. **Actionable Failure Signals**: Emits clear alerts and non-blocking logs if authentication or branch protection prevents creation.