### Spécification PO — [CAI-26] Titrage explicite des Pull Requests

**Priorité :** Must Have | **Release cible :** v1.2.0

#### User Story
**En tant que** développeur ou relecteur de la Software Factory,  
**Je veux que** le titre généré lors de l'ouverture d'une Pull Request reprenne l'identifiant et l'intitulé métier du ticket Linear,  
**Afin d'** identifier immédiatement le périmètre et la valeur livrée sans redondance inutile.

#### Critères d'Acceptation
- **Format standardisé :** Le titre suit le pattern `[<LINEAR_ID>] <Titre du ticket>` (sans duplication de préfixes ni suffixes type `#15 #15`).
- **Pré-remplissage automatique :** Au clic sur l'ouverture de la PR, le champ titre est automatiquement hydraté avec le nom assaini.
- **Lisibilité :** Aucun artefact de slugification brute (ex: tirets orphelins, casse incohérente) n'est présent.