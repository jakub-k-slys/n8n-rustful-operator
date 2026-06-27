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

  Scenario: s3 binary-data mode is shared across every role
    Given a Secret "pg-creds" exists with key "password" set to "s3cret"
    And a Secret "redis-creds" exists with key "password" set to "rs3cret"
    When I apply a Cluster "s3c" with s3 binary data bucket "n8n-bin"
    Then the Deployment "s3c-main" has env var "N8N_DEFAULT_BINARY_DATA_MODE" set to "s3"
    And the Deployment "s3c-main" has env var "N8N_EXTERNAL_STORAGE_S3_BUCKET_NAME" set to "n8n-bin"
    And the Deployment "s3c-worker" has env var "N8N_DEFAULT_BINARY_DATA_MODE" set to "s3"
    And the Deployment "s3c-worker" sources env var "N8N_EXTERNAL_STORAGE_S3_ACCESS_SECRET" from secret "s3-creds" key "access_secret"

  Scenario: database binary-data mode is set on every role
    Given a Secret "pg-creds" exists with key "password" set to "s3cret"
    And a Secret "redis-creds" exists with key "password" set to "rs3cret"
    When I apply a Cluster "bindb" with database binary data
    Then the Deployment "bindb-main" has env var "N8N_DEFAULT_BINARY_DATA_MODE" set to "database"
    And the Deployment "bindb-worker" has env var "N8N_DEFAULT_BINARY_DATA_MODE" set to "database"

  Scenario: filesystem binary-data with a shared RWX volume is mounted on every role
    Given a Secret "pg-creds" exists with key "password" set to "s3cret"
    And a Secret "redis-creds" exists with key "password" set to "rs3cret"
    When I apply a Cluster "binfs" with filesystem binary data shared size "1Gi"
    Then a PersistentVolumeClaim named "binfs-binary-data" exists with size "1Gi"
    And the Deployment "binfs-main" has env var "N8N_BINARY_DATA_STORAGE_PATH" set to "/home/node/binary-data"
    And the Deployment "binfs-main" mounts pvc "binfs-binary-data" at "/home/node/binary-data"
    And the Deployment "binfs-worker" mounts pvc "binfs-binary-data" at "/home/node/binary-data"

  Scenario: more than one main auto-enables multi-main (HA env + sticky sessions)
    Given a Secret "pg-creds" exists with key "password" set to "s3cret"
    And a Secret "redis-creds" exists with key "password" set to "rs3cret"
    When I apply a Cluster "mm" with 2 main replicas
    Then the Deployment "mm-main" has env var "N8N_MULTI_MAIN_SETUP_ENABLED" set to "true"
    And the Deployment "mm-main" has 2 replicas
    And the Service "mm-main" has session affinity "ClientIP"
    And a DestinationRule named "mm-main" exists within 60 seconds

  Scenario: multi-main leader key TTL is wired onto main
    Given a Secret "pg-creds" exists with key "password" set to "s3cret"
    And a Secret "redis-creds" exists with key "password" set to "rs3cret"
    When I apply a Cluster "mmt" with 2 main replicas and leader key ttl 20
    Then the Deployment "mmt-main" has env var "N8N_MULTI_MAIN_SETUP_KEY_TTL" set to "20"

  Scenario: scaling main back to one removes the DestinationRule
    Given a Secret "pg-creds" exists with key "password" set to "s3cret"
    And a Secret "redis-creds" exists with key "password" set to "rs3cret"
    When I apply a Cluster "mmgc" with 2 main replicas
    Then a DestinationRule named "mmgc-main" exists within 60 seconds
    When I update the Cluster "mmgc" to 1 main replicas
    Then a DestinationRule named "mmgc-main" is gone within 60 seconds

  Scenario: WEBHOOK_URL defaults to the main host without webhook processors
    Given a Secret "pg-creds" exists with key "password" set to "s3cret"
    And a Secret "redis-creds" exists with key "password" set to "rs3cret"
    When I apply a Cluster "wu-main" with main host "n8n.example.com"
    Then the Deployment "wu-main-main" has env var "WEBHOOK_URL" set to "http://n8n.example.com/"
    And the Deployment "wu-main-worker" has env var "WEBHOOK_URL" set to "http://n8n.example.com/"

  Scenario: WEBHOOK_URL uses the webhook host when webhook processors are configured
    Given a Secret "pg-creds" exists with key "password" set to "s3cret"
    And a Secret "redis-creds" exists with key "password" set to "rs3cret"
    When I apply a Cluster "wu-wh" with main host "n8n.example.com" and webhook host "hooks.example.com"
    Then the Deployment "wu-wh-main" has env var "WEBHOOK_URL" set to "http://hooks.example.com/"
    And the Deployment "wu-wh-worker" has env var "WEBHOOK_URL" set to "http://hooks.example.com/"
    And the Deployment "wu-wh-webhook" has env var "WEBHOOK_URL" set to "http://hooks.example.com/"

  Scenario: communityNodes sharedStorage mounts a shared volume on every role
    Given a Secret "pg-creds" exists with key "password" set to "s3cret"
    And a Secret "redis-creds" exists with key "password" set to "rs3cret"
    When I apply a Cluster "cnvol" with community nodes shared size "1Gi"
    Then a PersistentVolumeClaim named "cnvol-nodes" exists with size "1Gi"
    And the Deployment "cnvol-main" mounts pvc "cnvol-nodes" at "/home/node/.n8n/nodes"
    And the Deployment "cnvol-worker" mounts pvc "cnvol-nodes" at "/home/node/.n8n/nodes"

  Scenario: cluster-wide imagePullSecrets, resources and smtp apply to the main role
    Given a Secret "pg-creds" exists with key "password" set to "s3cret"
    And a Secret "redis-creds" exists with key "password" set to "rs3cret"
    When I apply a Cluster "blocks" with image pull secret "ghcr-secret", main resources and smtp
    Then the Deployment "blocks-main" has imagePullSecret "ghcr-secret"
    And the Deployment "blocks-main" requests cpu "200m" and limits memory "1Gi"
    And the Deployment "blocks-main" has env var "N8N_EMAIL_MODE" set to "smtp"

  Scenario: communityNodes packages are managed declaratively on every role
    Given a Secret "pg-creds" exists with key "password" set to "s3cret"
    And a Secret "redis-creds" exists with key "password" set to "rs3cret"
    When I apply a Cluster "cnodes" with community package "n8n-nodes-foo"
    Then the Deployment "cnodes-main" has env var "N8N_COMMUNITY_PACKAGES_ENABLED" set to "true"
    And the Deployment "cnodes-main" has env var "N8N_COMMUNITY_PACKAGES_MANAGED_BY_ENV" set to "true"
    And the Deployment "cnodes-worker" has env var "N8N_COMMUNITY_PACKAGES_MANAGED_BY_ENV" set to "true"

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

  Scenario: secureCookie sets N8N_SECURE_COOKIE on every role
    Given a Secret "pg-creds" exists with key "password" set to "s3cret"
    And a Secret "redis-creds" exists with key "password" set to "rs3cret"
    When I apply a Cluster "cluster-cookie" with secureCookie false
    Then the Deployment "cluster-cookie-main" has env var "N8N_SECURE_COOKIE" set to "false"
    And the Deployment "cluster-cookie-worker" has env var "N8N_SECURE_COOKIE" set to "false"

  Scenario: A role's extraEnv overrides the cluster-wide secureCookie
    Given a Secret "pg-creds" exists with key "password" set to "s3cret"
    And a Secret "redis-creds" exists with key "password" set to "rs3cret"
    When I apply a Cluster "override" with secureCookie true and main extraEnv "N8N_SECURE_COOKIE"="false"
    Then the Deployment "override-main" has env var "N8N_SECURE_COOKIE" set to "false"
    And the Deployment "override-worker" has env var "N8N_SECURE_COOKIE" set to "true"
