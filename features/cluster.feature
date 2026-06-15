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

  Scenario: Cluster main.persistence provisions a PVC mounted on the main pod only
    Given a Secret "pg-creds" exists with key "password" set to "s3cret"
    And a Secret "redis-creds" exists with key "password" set to "rs3cret"
    When I apply a Cluster "pv-cluster" with main persistence size "2Gi"
    Then a PersistentVolumeClaim named "pv-cluster-main-data" exists with size "2Gi"
    And the Deployment "pv-cluster-main" mounts pvc "pv-cluster-main-data" at "/home/node/.n8n"

  Scenario: Cluster auto-generates an encryption Secret shared by all roles
    Given a Secret "pg-creds" exists with key "password" set to "s3cret"
    And a Secret "redis-creds" exists with key "password" set to "rs3cret"
    When I apply a Cluster "auto-key" backed by Postgres "pg.example.com" and Redis "redis.example.com" with 1 workers and webhooks
    Then a Secret named "auto-key-encryption-key" eventually exists with a non-empty key "encryption_key"
    And the Deployment "auto-key-main" sources env var "N8N_ENCRYPTION_KEY" from secret "auto-key-encryption-key" key "encryption_key"
    And the Deployment "auto-key-worker" sources env var "N8N_ENCRYPTION_KEY" from secret "auto-key-encryption-key" key "encryption_key"
    And the Deployment "auto-key-webhook" sources env var "N8N_ENCRYPTION_KEY" from secret "auto-key-encryption-key" key "encryption_key"

  Scenario: Cluster honours a user-provided encryption Secret
    Given a Secret "pg-creds" exists with key "password" set to "s3cret"
    And a Secret "redis-creds" exists with key "password" set to "rs3cret"
    And a Secret "byo-cluster-key" exists with key "encryption_key" set to "shared-secret"
    When I apply a Cluster "byo-cluster" with encryption key from secret "byo-cluster-key" key "encryption_key"
    Then the Deployment "byo-cluster-main" sources env var "N8N_ENCRYPTION_KEY" from secret "byo-cluster-key" key "encryption_key"
    And no Secret named "byo-cluster-encryption-key" exists

  Scenario: Workers do not get a Service
    Given a Secret "pg-creds" exists with key "password" set to "s3cret"
    And a Secret "redis-creds" exists with key "password" set to "rs3cret"
    When I apply a Cluster "no-worker-svc" backed by Postgres "pg.example.com" and Redis "redis.example.com" with 1 workers and webhooks
    Then a Deployment named "no-worker-svc-worker" exists in namespace "default" within 60 seconds
    And no Service named "no-worker-svc-worker" exists

  Scenario: Worker and webhook pods run the matching n8n subcommand
    Given a Secret "pg-creds" exists with key "password" set to "s3cret"
    And a Secret "redis-creds" exists with key "password" set to "rs3cret"
    When I apply a Cluster "cmd" backed by Postgres "pg.example.com" and Redis "redis.example.com" with 1 workers and webhooks
    Then the Deployment "cmd-worker" runs command "n8n worker"
    And the Deployment "cmd-webhook" runs command "n8n webhook"

  Scenario: Cluster main can be exposed via HTTPRoute
    Given a Secret "pg-creds" exists with key "password" set to "s3cret"
    And a Secret "redis-creds" exists with key "password" set to "rs3cret"
    When I apply a Cluster "rt" with main httpRoute gateway "shared-gw" namespace "default" and host "rt.example.com"
    Then an HTTPRoute named "rt-main" exists with host "rt.example.com" within 60 seconds

  Scenario: Cluster main can be exposed via Ingress
    Given a Secret "pg-creds" exists with key "password" set to "s3cret"
    And a Secret "redis-creds" exists with key "password" set to "rs3cret"
    When I apply a Cluster "ing" with main ingress class "nginx" and host "main.example.com"
    Then an Ingress named "ing-main" exists with host "main.example.com" within 60 seconds

  Scenario: Per-role image overrides
    Given a Secret "pg-creds" exists with key "password" set to "s3cret"
    And a Secret "redis-creds" exists with key "password" set to "rs3cret"
    When I apply a Cluster "img-ovr" with main image "alpine:3.18" and worker image "alpine:3.19"
    Then the Deployment "img-ovr-main" runs image "alpine:3.18"
    And the Deployment "img-ovr-worker" runs image "alpine:3.19"

  Scenario: Redis password and prefix wire into the queue env
    Given a Secret "pg-creds" exists with key "password" set to "s3cret"
    And a Secret "redis-creds" exists with key "password" set to "rs3cret"
    When I apply a Cluster "redis-env" with Redis prefix "n8n-prod"
    Then the Deployment "redis-env-main" sources env var "QUEUE_BULL_REDIS_PASSWORD" from secret "redis-creds" key "password"
    And the Deployment "redis-env-main" has env var "QUEUE_BULL_PREFIX" set to "n8n-prod"

  Scenario: Cluster status reports replica counts per role
    Given a Secret "pg-creds" exists with key "password" set to "s3cret"
    And a Secret "redis-creds" exists with key "password" set to "rs3cret"
    When I apply a Cluster "status" backed by Postgres "pg.example.com" and Redis "redis.example.com" with 3 workers and webhooks
    Then the Cluster "status" has status mainReplicas 1 workerReplicas 3 webhookReplicas 1

  Scenario: Workers autoscale via HPA and operator stops managing spec.replicas
    Given a Secret "pg-creds" exists with key "password" set to "s3cret"
    And a Secret "redis-creds" exists with key "password" set to "rs3cret"
    When I apply a Cluster "hpa" with worker autoscaling min 1 max 3
    Then a HorizontalPodAutoscaler named "hpa-worker" exists with min 1 max 3 within 60 seconds
    And the HorizontalPodAutoscaler "hpa-worker" targets Deployment "hpa-worker"

  Scenario: Dropping worker autoscaling removes the HPA
    Given a Secret "pg-creds" exists with key "password" set to "s3cret"
    And a Secret "redis-creds" exists with key "password" set to "rs3cret"
    And a Cluster "hpa-drop" exists with worker autoscaling min 1 max 3
    When I update the Cluster "hpa-drop" to have no worker autoscaling
    Then the HorizontalPodAutoscaler "hpa-drop-worker" is gone within 60 seconds

  Scenario: Dropping the webhooks block removes the webhook Deployment
    Given a Secret "pg-creds" exists with key "password" set to "s3cret"
    And a Secret "redis-creds" exists with key "password" set to "rs3cret"
    Given a Cluster "wh-drop" exists with webhooks
    When I update the Cluster "wh-drop" to have no webhooks
    Then the Deployment "wh-drop-webhook" is gone within 60 seconds
