### Test Factory (QA) — BDD & Integration Suite

```gherkin
Feature: [CAI-18] L'agent PR doit être capable d'ouvrir la PR automatiquement contre la branche par défaut —  (v1.3.0)
Scenario: Nominal execution validated by Connected Test Factory
Given the repository is at release "v1.3.0"
When a client executes the workflow for "[CAI-18] L'agent PR doit être capable d'ouvrir la PR automatiquement contre la branche par défaut — "
Then the response status is 200 OK
And rollback_safe is true

```