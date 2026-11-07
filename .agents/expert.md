### Business Expert (Expert Métier) — Domain Rules

- **Invariant D1 (Idempotency)**: Every command for *"[CAI-23] Les agents répondent toujours en EN alors que je su"* carries a deterministic idempotency key.
- **Invariant D2 (Consistency)**: Domain events emit only after validation succeeds.
- **Invariant D3 (Reversibility)**: State transitions support instant OPS rollback.