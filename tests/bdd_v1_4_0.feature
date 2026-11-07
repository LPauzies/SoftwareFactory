Feature: [CAI-23] Les agents répondent toujours en EN alors que je suis en FR —  (v1.4.0)
Scenario: Nominal execution validated by Connected Test Factory
Given the repository is at release "v1.4.0"
When a client executes the workflow for "[CAI-23] Les agents répondent toujours en EN alors que je suis en FR — "
Then the response status is 200 OK
And rollback_safe is true
