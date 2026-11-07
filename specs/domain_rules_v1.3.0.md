### Business Expert (Expert Métier) — Domain Rules

- **Invariant D1 (Idempotency)**: Every command for *"[CAI-18] L'agent PR doit être capable d'ouvrir la PR automat"* carries a deterministic idempotency key.
- **Invariant D2 (Consistency)**: Domain events emit only after validation succeeds.
- **Invariant D3 (Reversibility)**: State transitions support instant OPS rollback.