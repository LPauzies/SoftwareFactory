### Spécification BDD & Plan de Test Factory (QA) — Ticket [CAI-26]

**Scénario Gherkin :**
```gherkin
Fonctionnalité: Génération explicite du titre de Pull Request
  En tant qu'opérateur de la Software Factory
  Afin d'identifier immédiatement le sujet traité
  Je veux que le titre de la PR contienne la clé et le titre du ticket

  Scénario: Titrage nominal d'une PR depuis Linear
    Étant donné un ticket Linear "CAI-26" avec le titre "Nommage explicite de la PR"
    Quand le pipeline de publication déclenche l'ouverture de la PR
    Alors le titre de la PR généré doit être "[CAI-26] Nommage explicite de la PR"
    Et ne doit pas contenir de duplication d'identifiant opaque
```

**Plan d'automatisation des tests d'intégration :**
- **Test d'intégration backend (`backend/tests/pr_naming_integration_test.rs`)** : Instanciation du client mock GitHub et passage d'un contexte de ticket simulé à `orchestrator.rs` pour vérifier l'intégrité de la payload HTTP (`title`).
- **Validation non-régression** : Contrôle systématique en pipeline CI bloquant si le format ne respecte pas l'expression régulière attendue `^\[CAI-\d+\] .+$`.