Feature: n8n operator reconciles Cluster custom resources

  Background:
    Given a kind cluster with the operator installed

  Scenario: Cluster creates main, worker and webhook Deployments
    Given a Secret "pg-creds" exists with key "password" set to "s3cret"
    And a Secret "redis-creds" exists with key "password" set to "rs3cret"
    When I apply a Cluster "qm" backed by Postgres "pg.example.com" and Redis "redis.example.com" with 3 workers and webhooks
    Then a Deployment named "qm-main" exists in namespace "default" within 60 seconds
    And a Deployment named "qm-worker" exists in namespace "default" within 60 seconds
    And a Deployment named "qm-webhook" exists in namespace "default" within 60 seconds
    And the Deployment "qm-main" has env var "EXECUTIONS_MODE" set to "queue"
    And the Deployment "qm-worker" has env var "EXECUTIONS_MODE" set to "queue"
    And the Deployment "qm-worker" has env var "QUEUE_HEALTH_CHECK_ACTIVE" set to "true"
    And the Deployment "qm-webhook" has env var "N8N_DISABLE_PRODUCTION_MAIN_PROCESS" set to "true"
    And the Deployment "qm-main" has env var "QUEUE_BULL_REDIS_HOST" set to "redis.example.com"
    And the Deployment "qm-main" has env var "DB_TYPE" set to "postgresdb"

  Scenario: Cluster rejects sqlite (queue mode needs shared DB)
    When I apply a Cluster "bad-cluster" with sqlite database
    Then the Cluster "bad-cluster" never reaches status.ready=true within 15 seconds

  Scenario: Dropping the webhooks block removes the webhook Deployment
    Given a Secret "pg-creds" exists with key "password" set to "s3cret"
    And a Secret "redis-creds" exists with key "password" set to "rs3cret"
    Given a Cluster "wh-drop" exists with webhooks
    When I update the Cluster "wh-drop" to have no webhooks
    Then the Deployment "wh-drop-webhook" is gone within 60 seconds
