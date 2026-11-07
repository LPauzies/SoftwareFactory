### Architecture Specification — `[CAI-18]` Auto-PR Default Branch

#### 1. Modular Boundaries & Factory Homogeneity
- **`ScmBranchResolver` (Core Git Module)**: Resolves the canonical target branch via `refs/remotes/origin/HEAD`, falling back to SCM provider metadata (`default_branch`) if uninitialized locally. Exposed as a pure, stateless utility across all WAGNER agents.
- **`PullRequestGateway` (SCM Adapter)**: Encapsulates PR lifecycle operations (`findOpenPr`, `createPr`, `markDraft`) behind a provider-agnostic interface, keeping the PR Agent decoupled from transport specifics.

#### 2. Idempotent & Rollback-Safe State Machine
1. **`RESOLVED`**: Resolve `base_branch` and verify `head_branch` divergence (`commits > 0`). Abort cleanly if no diff exists.
2. **`PUSHED`**: Push `head_branch` to `origin` atomically.
3. **`PR_OPENED` (Idempotent Gate)**: Query `findOpenPr(head, base)`. If an open PR exists, update its metadata and return its URL; otherwise, create the PR targeting `base_branch` (respecting the configured `draft` policy).

#### 3. Rollback & Recovery Contract
If PR creation fails after `PUSHED`, the state remains re-entrant; subsequent retries reuse the pushed ref and idempotently converge on `PR_OPENED` without duplicate PRs.