### Architecture & Design Modulaire — `[CAI-26]` (v1.2.0)

#### 1. Frontières Modulaires & Responsabilités
Pour garantir l'homogénéité de la Software Factory WAGNER sans couplage fort entre Linear et GitHub :
- **Module `TicketContext` (Domaine)** : Expose une structure immuable contenant `id` (`CAI-XX`) et `title` (intitulé métier nettoyé et tronqué à 72 caractères).
- **Helper Pur `PRTitleFormatter`** : Fonction déterministe sans effet de bord qui transforme le `TicketContext` en chaîne normalisée : `[CAI-XX] <Intitulé du ticket>`.
- **Adaptateur VCS (`GitHubPRAdapter`)** : Consomme uniquement le titre pré-formaté via ses paramètres d'entrée (`--title`), éliminant la génération implicite basée sur le nom de branche (`Factory cai 22...`).

#### 2. Contrat d'Interface & Stratégie Rollback-Safe
- **Validation & Fallback** : Si l'intitulé métier est indisponible lors de l'exécution, le formateur bascule automatiquement sur `[CAI-XX] Automated Factory PR` sans bloquer le pipeline.
- **Réversibilité** : Modification strictement sans état (*stateless*) et rétrocompatible. Aucun schéma de persistance n'étant altéré, un rollback vers la version précédente restaure instantanément le comportement par défaut de `gh pr create`.