## Pull Request : [CAI-27] Stabilisation & immutabilité des définitions d'agents

**Branche source** : `feature/CAI-27-agent-immutability`  
**Branche cible** : `master` | **Release associée** : `v1.1.0`

### Description des modifications
Cette PR verrouille l'immutabilité des définitions d'agents dans l'usine logicielle WAGNER. Elle supprime l'injection de numéros de tickets Linear ou métadonnées contextuelles éphémères dans les définitions permanentes, évitant ainsi les faux conflits Git et garantissant des structures d'agents déterministes.

### Checklist de Contrôle & Revue Humaine
- [ ] **Immutabilité** : Les agents ne portent plus d'historique ou de métadonnées de tickets dans leurs fichiers de base.
- [ ] **Déterminisme** : Les définitions d'agents restent strictement inchangées hors mandat explicite de refactorisation.
- [ ] **Arbre Git propre** : Aucun artefact de session ou variable d'exécution n'est tracké dans la PR.
- [ ] **Validation WAGNER** : Conformité validée avec la suite de tests unitaires et linters de l'usine logicielle.