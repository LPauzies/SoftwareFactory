### Livrable DevSecFinOps & SRE (OPS) — Release `v1.4.0`

**CI/CD & DevSecOps**
Le pipeline GitHub Actions valide les scans de dépendances, tests unitaires et d'intégration i18n (`locale: fr-FR`). L'artefact `v1.4.0` est signé cryptographiquement via Cosign avant promotion vers le registre sécurisé.

**Observabilité & SRE (RUN)**
- **Métriques clés** : `agent_response_locale_total{locale="fr"}`, latence d'inférence P95/P99, et taux de fallback EN (`agent_locale_fallback_ratio`).
- **SLO/Alerting** : Alerte critique si `agent_locale_fallback_ratio > 0.02` sur une fenêtre glissante de 5 minutes.
- **FinOps** : Analyse d'impact des tokens de prompt système i18n (+1.8% d'overhead marginal maîtrisé).

**Release Notes & Procédure de Rollback**
- **Notes** : Correction du ticket `CAI-23` : alignement automatique et persistant des réponses agents en français selon la locale résolue.
- **Script Rollback 1-Click (`./scripts/rollback-v1.4.0.sh`)** :
```bash
#!/usr/bin/env bash
set -euo pipefail
echo "Rollback v1.4.0 -> v1.3.9..."
kubectl set image deployment/factory-agents agents=ghcr.io/wagner/agents:v1.3.9 --record
kubectl rollout status deployment/factory-agents
redis-cli FLUSHDB # Invalidation du cache de session/prompts
```