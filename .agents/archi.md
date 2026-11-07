### Architecture Système — [CAI-23] Support Multilingue & Homogénéité Locale (v1.4.0)

Pour garantir que tous les agents de la Software Factory WAGNER répondent dans la langue de l'utilisateur (FR) tout en préservant l'homogénéité du système :

1. **Contrat d'Interface Unifié (`LocaleContext`)** :
   L'orchestrateur central résout la langue dès l'ingestion du ticket/prompt (`locale: "fr-FR"`) et l'injecte dans l'en-tête de prompt système de chaque rôle (PO, EXPERT, ARCHI, DEV, QA, OPS). Aucun agent ne redétecte la langue isolément.

2. **Frontières Modulaires Propres** :
   - **Couche Protocole (Invariante)** : Les balises de contrôle (`<thinking>`, `<question>`), les clés JSON d'état et les invariants d'idempotence (D1) demeurent strictement canoniques pour ne pas casser les parseurs de l'usine.
   - **Couche Présentation (Localisée)** : Les raisonnements internes, questions opérateur et livrables Markdown sont générés nativement dans la `locale` active.

3. **Transition d'État & Rollback-Safe** :
   Le champ `locale` est optionnel avec fallback automatique par détection du prompt utilisateur. En cas de rollback vers la version précédente, l'absence d'instruction locale n'altère ni la persistance des états ni la compatibilité des pipelines CI/CD existants.