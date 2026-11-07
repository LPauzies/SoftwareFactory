### Livrable DevSecFinOps & SRE — Wagner Factory (`v1.2.0`)

#### 1. Pipeline CI/CD & Déploiement
Le pipeline automatise la release `v1.2.0` suite à la validation QA :
* **Build & SecOps :** Audit SAST/Secrets sans surcoût infrastructure (FinOps).
* **Release :** Tagging Git `v1.2.0` et publication automatique des artefacts.

#### 2. Release Notes
* **Feature [CAI-26] :** Normalisation explicite du titre et du template des Pull Requests (ex. `[CAI-26] Titre métier explicite` au lieu de `Factory cai 22 - #15 #15`).
* **Impact :** Lisibilité accrue dans GitHub/Linear et simplification de la traçabilité.

#### 3. Observabilité du RUN
* **SLO :** 99.9% de succès sur la génération des métadonnées de PR.
* **Alerting :** Notification immédiate si des titres génériques ou formats anormaux sont détectés via webhook.

#### 4. Procédure de Rollback
Script d'urgence en cas d'anomalie :
```bash
#!/bin/bash
set -e
echo "[ROLLBACK] Rétablissement de la version stable antérieure..."
git checkout v1.1.9
git push origin HEAD --force-with-lease
echo "[ROLLBACK] v1.1.9 réactivée avec succès."
```