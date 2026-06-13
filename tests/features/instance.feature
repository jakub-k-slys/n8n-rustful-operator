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
