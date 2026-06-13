Feature: n8n operator reconciles Instance custom resources

  Background:
    Given a kind cluster with the operator installed

  Scenario: Operator creates Deployment and Service for a new Instance
    When I apply an Instance "smoke" with image "nginx:alpine"
    Then a Deployment named "smoke" exists in namespace "default" within 60 seconds
    And a Service named "smoke" exposes port 5678
    And the Instance "smoke" has status.ready set to true within 60 seconds
    And the Instance "smoke" has the finalizer "instances.n8n.slys.dev"

  Scenario: Operator removes the Instance on delete
    Given an Instance "to-delete" exists
    When I delete the Instance "to-delete"
    Then the Instance "to-delete" is gone within 60 seconds

  Scenario: Service defaults to ClusterIP
    When I apply an Instance "svc-default" with image "nginx:alpine"
    Then a Deployment named "svc-default" exists in namespace "default" within 60 seconds
    And the Service "svc-default" has type "ClusterIP"

  Scenario: Service can be exposed as NodePort
    When I apply an Instance "svc-nodeport" with image "nginx:alpine" and service type "NodePort"
    Then a Deployment named "svc-nodeport" exists in namespace "default" within 60 seconds
    And the Service "svc-nodeport" has type "NodePort"

  Scenario: Operator auto-generates an encryption key Secret with an owner reference
    When I apply an Instance "with-key" with image "nginx:alpine"
    Then a Secret named "with-key-encryption-key" eventually exists with a non-empty key "encryption_key"
    And the Secret "with-key-encryption-key" is owned by the Instance "with-key"
    And the Deployment "with-key" sources env var "N8N_ENCRYPTION_KEY" from secret "with-key-encryption-key" key "encryption_key"

  Scenario: Operator uses a user-provided encryption key Secret
    Given a Secret "byo-secrets" exists with key "encryption_key" set to "static-value"
    When I apply an Instance "byo-key" with image "nginx:alpine" and encryption key from secret "byo-secrets" key "encryption_key"
    Then the Deployment "byo-key" sources env var "N8N_ENCRYPTION_KEY" from secret "byo-secrets" key "encryption_key"
    And no Secret named "byo-key-encryption-key" exists

  Scenario: Operator creates an Ingress when configured
    When I apply an Instance "with-ingress" with ingress class "nginx" and host "ingress.example.com"
    Then an Ingress named "with-ingress" exists with host "ingress.example.com" within 60 seconds

  Scenario: Operator removes the Ingress when networking is dropped from spec
    Given an Instance "drop-ingress" exists with ingress class "nginx" and host "ingress.example.com"
    When I update the Instance "drop-ingress" to have no networking
    Then the Ingress "drop-ingress" is gone within 60 seconds

  Scenario: Operator rejects setting both ingress and httpRoute
    When I apply an Instance "both" with both ingress and httpRoute
    Then the Instance "both" never reaches status.ready=true within 15 seconds
    And no Ingress named "both" exists
