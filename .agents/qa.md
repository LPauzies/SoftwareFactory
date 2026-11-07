### Livrable QA & Test Factory — Ticket [CAI-27]

Dans le cadre du jalon `v1.1.0`, la Test Factory verrouille l'étanchéité entre la couche statique (archétypes d'agents) et le runtime contextuel :

#### Spécification BDD Gherkin (`features/agent_immutability.feature`)
```gherkin
# language: fr
Fonctionnalité: Immuabilité des définitions d'agents en cours d'exécution de ticket

  Scénario: Préservation de l'archétype lors du traitement d'un ticket standard
    Étant donné un agent défini dans son fichier statique d'archétype
    Et que le ticket en cours de traitement est "CAI-27"
    Quand l'orchestrateur instancie et exécute la tâche de l'agent
    Alors le fichier de configuration de l'agent ne doit subir aucune modification
    Et aucune référence à l'identifiant "CAI-27" ne doit être persistée dans sa définition
    Et l'empreinte SHA-256 de l'archétype doit rester strictement identique après exécution
```

#### Suite de Tests d'Intégration
- **`test_agent_definition_immutability()`** : Exécute un workflow complet avec injection de métadonnées de ticket et asserte l'état invariant (`git diff --exit-code`) sur le dossier des archétypes.
- **`test_context_runtime_isolation()`** : Vérifie que l'historique de contexte et les identifiants de tâches sont consignés exclusivement dans la mémoire volatile de session ou le journal d'audit éphémère.