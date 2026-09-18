Feature: Assistant provisions a self-hosted sandbox stack

  Background:
    Given a kind cluster with the operator installed

  Scenario: the sandbox stack comes up, the target Secret is written, and cleanup removes it
    Given a privileged namespace "n8n-sandbox"
    And a Secret "assistant-model-key" exists with key "ANTHROPIC_API_KEY" set to "sk-test"
    When I apply an Assistant named "e2e" targeting Cluster "e2e-cluster" with sandbox namespace "n8n-sandbox" using model key secret "assistant-model-key" key "ANTHROPIC_API_KEY"
    Then the Job "default-e2e-sandbox-certs" in namespace "n8n-sandbox" completes within 180 seconds
    And the Secret "default-e2e-sandbox-tls-api" exists in namespace "n8n-sandbox" within 60 seconds
    And the Secret "default-e2e-sandbox-tls-runner" exists in namespace "n8n-sandbox" within 60 seconds
    And the Secret "default-e2e-sandbox-tls-api" in namespace "n8n-sandbox" has no key "ca.key"
    And a Deployment named "default-e2e-sandbox-api" exists in namespace "n8n-sandbox" within 60 seconds
    And a Deployment named "default-e2e-sandbox-runner-1" exists in namespace "n8n-sandbox" within 60 seconds
    And the Deployment "default-e2e-sandbox-api" in namespace "n8n-sandbox" becomes available within 300 seconds
    And the Deployment "default-e2e-sandbox-runner-1" in namespace "n8n-sandbox" becomes available within 300 seconds
    And the Secret "e2e-cluster-instance-ai" contains key "N8N_SANDBOX_SERVICE_API_KEY"
    And the Secret "e2e-cluster-instance-ai" contains key "N8N_ENABLED_MODULES"
    When I delete the Assistant named "e2e"
    Then no Deployments remain in namespace "n8n-sandbox" for assistant "e2e" within 60 seconds
    And no Secrets remain in namespace "n8n-sandbox" for assistant "e2e" within 60 seconds
