## Rôle Métier : Spécification du Domaine & Règles Invariants [CAI-22]

### 1. Langage Ubiquitaire (Ubiquitous Language)
* **Ponytail Full Mode** : Standard de développement appliquant le principe du moindre changement suffisant (*Minimal Viable Diff*), interdisant tout code préemptif, sur-architecturé ou non requis par le besoin immédiat.
* **Dette Maîtrisée (Shortcut Tagging)** : Toute simplification intentionnelle différant une optimisation secondaire doit être formalisée via le marqueur `shortcut:`.

### 2. Invariants & Règles Métier
* **Invariant R1 (KISS Strict)** : Aucune abstraction (couche, wrapper, interface générique) n'est autorisée si elle n'est pas exploitée par au moins deux composants réels distincts.
* **Invariant R2 (Auditabilité)** : Toute tâche d'implémentation doit attester de l'activation explicite des contraintes Ponytail Full.
* **Règle R3 (Contrôle aux Limites)** : Les cas limites (*edge cases*) doivent être résolus par le traitement direct le plus simple, sans ajout de dépendances tierces superflues.

### 3. Critères d'Acceptation Métier (Gherkin)
* **Scénario** : Soumission d'une implémentation
  * **Étant donné** un ticket fonctionnel assigné à la Factory,
  * **Quand** l'agent Developer produit la solution technique,
  * **Alors** le diff doit refléter le chemin critique direct, avec justification explicite de chaque ajout selon la politique Ponytail Full.