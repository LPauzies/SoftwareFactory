Feature: [CAI-22] Assure toi de tout le temps utiliser le plugin ponytail full pour les implémentations —  (v1.3.0)
Scenario: Nominal execution validated by Connected Test Factory
Given the repository is at release "v1.3.0"
When a client executes the workflow for "[CAI-22] Assure toi de tout le temps utiliser le plugin ponytail full pour les implémentations — "
Then the response status is 200 OK
And rollback_safe is true
