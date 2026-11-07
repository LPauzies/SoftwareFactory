### Business Expert (Expert Métier) — Domain Rules

- **Invariant D1 (Idempotency)**: Every command for *"[CAI-22] Assure toi de tout le temps utiliser le plugin pony"* carries a deterministic idempotency key.
- **Invariant D2 (Consistency)**: Domain events emit only after validation succeeds.
- **Invariant D3 (Reversibility)**: State transitions support instant OPS rollback.