### Livrable QA & Test Factory — Release v1.4.0 ([CAI-23])

Pour garantir la conformité métier et technique de la localisation, la **Test Factory** raccorde une suite d'intégration automatisée et formalise les critères d'acceptation en Gherkin BDD :

```gherkin
Fonctionnalité: Réponse des agents selon la langue de l'opérateur
  Contexte:
    Etant donné un opérateur configuré avec la locale "fr-FR"

  Scénario: Interaction standard en français
    Quand l'opérateur soumet une consigne en français dans le run
    Alors chaque agent sollicité (PO, Architecte, DEV) répond en français
    Et les invariants techniques (D1) demeurent inchangés

  Scénario: Préservation du jargon technique
    Quand l'opérateur soumet une consigne technique avec des termes en anglais
    Alors la structure globale de réponse est en français
    Et les termes techniques clés sont préservés sans altération syntaxique
```

**Actions d'automatisation validées :**
- **Tests d'intégration** : Validation bout-en-bout du pipeline avec injection de payloads `fr` et vérification des tokens de sortie.
- **Raccordement CI/CD** : Intégration du job de contrôle de locale dans la barrière qualité avant validation du run.