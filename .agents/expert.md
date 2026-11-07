### Business Rules & Living Documentation: Autonomous PR Agent Targeting (`CAI-18`)

#### 1. Ubiquitous Domain Language
* **Target Default Branch**: The repository's canonical branch resolved dynamically via remote HEAD inspection (`origin/HEAD` / API reference), never hardcoded.
* **PR Publication Mode**: The lifecycle status assigned upon PR creation (`DRAFT` vs. `READY_FOR_REVIEW`).
* **Source Head Reference**: The unique, pushed feature/fix branch containing factory-generated commits.

#### 2. Core Business Invariants
1. **Dynamic Target Resolution**: The agent must query the remote default branch; targeting non-default branches without explicit operator override is prohibited.
2. **Draft Guardrail Invariant**: Unless explicitly configured by operator policy, automated PRs must open in `Draft` status to prevent unsolicited CI/CD triggers and developer spam.
3. **Idempotent Submission**: If an open PR already exists for the identical source head and target branch, update the PR description/context rather than opening duplicates.

#### 3. Edge Cases & Validation Rules
* **Ambiguous Default Ref**: If remote HEAD resolution fails, abort before PR creation with a clear diagnostic event.
* **Branch Desynchronization**: Reject PR creation if the source branch contains zero differing commits against the default branch.