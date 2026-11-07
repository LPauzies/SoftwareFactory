### Spécification Métier — Titrage Sémantique des Pull Requests

#### Langage Ubiquitaire (Ubiquitous Language)
* **Ticket Source** : Entité de travail Linear identifiée par une clé canonique (`CAI-XX`) et un intitulé métier.
* **Titre de PR Dérivé** : Intitulé contextuel et synthétique de la Pull Request reflétant fidèlement l'intention de la livraison.
* **Traçabilité Directe** : Lien textuel univoque garantissant la lisibilité immédiate sans navigation externe.

#### Invariants Métier
1. Tout titre de PR généré inclut impérativement l'identifiant exact du ticket (`CAI-XX`).
2. Tout doublon d'identifiant ou numéro arbitraire redondant (ex: `#15 #15`) est proscrit.
3. Le sujet métier doit être tronqué élégamment à 72 caractères sans altération sémantique.

#### Règles de Validation
* **Génération** : Le template applique le format `[CLÉ] Résumé métier explicite`.
* **Conformité** : Rejet de toute création si l'intitulé se résume au nom du dépôt ou à un placeholder générique.