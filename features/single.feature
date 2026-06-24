Feature: n8n operator reconciles Single custom resources

  Background:
    Given a kind cluster with the operator installed

  Scenario: Operator creates Deployment and Service for a new Single
    When I apply a Single "smoke" with image "nginx:alpine"
    Then a Deployment named "smoke" exists in namespace "default" within 60 seconds
    And a Service named "smoke" exposes port 5678
    And the Single "smoke" has status.ready set to true within 60 seconds
    And the Single "smoke" has the finalizer "singles.n8n.slys.dev"

  Scenario: Operator removes the Single on delete
    Given a Single "to-delete" exists
    When I delete the Single "to-delete"
    Then the Single "to-delete" is gone within 60 seconds

  Scenario: Service defaults to ClusterIP
    When I apply a Single "svc-default" with image "nginx:alpine"
    Then a Deployment named "svc-default" exists in namespace "default" within 60 seconds
    And the Service "svc-default" has type "ClusterIP"

  Scenario: Service can be exposed as NodePort
    When I apply a Single "svc-nodeport" with image "nginx:alpine" and service type "NodePort"
    Then a Deployment named "svc-nodeport" exists in namespace "default" within 60 seconds
    And the Service "svc-nodeport" has type "NodePort"

  Scenario: Operator auto-generates an encryption key Secret with an owner reference
    When I apply a Single "with-key" with image "nginx:alpine"
    Then a Secret named "with-key-encryption-key" eventually exists with a non-empty key "encryption_key"
    And the Secret "with-key-encryption-key" is owned by the Single "with-key"
    And the Deployment "with-key" sources env var "N8N_ENCRYPTION_KEY" from secret "with-key-encryption-key" key "encryption_key"

  Scenario: Operator uses a user-provided encryption key Secret
    Given a Secret "byo-secrets" exists with key "encryption_key" set to "static-value"
    When I apply a Single "byo-key" with image "nginx:alpine" and encryption key from secret "byo-secrets" key "encryption_key"
    Then the Deployment "byo-key" sources env var "N8N_ENCRYPTION_KEY" from secret "byo-secrets" key "encryption_key"
    And no Secret named "byo-key-encryption-key" exists

  Scenario: Operator creates an Ingress when configured
    When I apply a Single "with-ingress" with ingress class "nginx" and host "ingress.example.com"
    Then an Ingress named "with-ingress" exists with host "ingress.example.com" within 60 seconds

  Scenario: Operator removes the Ingress when networking is dropped from spec
    Given a Single "drop-ingress" exists with ingress class "nginx" and host "ingress.example.com"
    When I update the Single "drop-ingress" to have no networking
    Then the Ingress "drop-ingress" is gone within 60 seconds

  Scenario: Operator rejects setting both ingress and httpRoute
    When I apply a Single "both" with both ingress and httpRoute
    Then the Single "both" never reaches status.ready=true within 15 seconds
    And no Ingress named "both" exists

  Scenario: Default Single has no database env vars
    When I apply a Single "db-default" with image "nginx:alpine"
    Then a Deployment named "db-default" exists in namespace "default" within 60 seconds
    And the Deployment "db-default" has no env var "DB_TYPE"

  Scenario: Postgres database wires host, user, schema and password from a Secret
    Given a Secret "pg-creds" exists with key "password" set to "s3cret"
    When I apply a Single "pg" with Postgres host "pg.example.com" port 5432 database "n8n" user "n8n" password from secret "pg-creds" key "password" schema "public" pool size 7
    Then the Deployment "pg" has env var "DB_TYPE" set to "postgresdb"
    And the Deployment "pg" has env var "DB_POSTGRESDB_HOST" set to "pg.example.com"
    And the Deployment "pg" has env var "DB_POSTGRESDB_PORT" set to "5432"
    And the Deployment "pg" has env var "DB_POSTGRESDB_DATABASE" set to "n8n"
    And the Deployment "pg" has env var "DB_POSTGRESDB_USER" set to "n8n"
    And the Deployment "pg" has env var "DB_POSTGRESDB_SCHEMA" set to "public"
    And the Deployment "pg" has env var "DB_POSTGRESDB_POOL_SIZE" set to "7"
    And the Deployment "pg" sources env var "DB_POSTGRESDB_PASSWORD" from secret "pg-creds" key "password"

  Scenario: Postgres with SSL CA mounts the cert and points env var at the file
    Given a Secret "pg-creds" exists with key "password" set to "s3cret"
    And a Secret "pg-ca" exists with key "ca.crt" set to "fake-ca-pem"
    When I apply a Single "pg-ssl" with Postgres host "pg.example.com" database "n8n" user "n8n" password from secret "pg-creds" key "password" and SSL CA from secret "pg-ca" key "ca.crt"
    Then the Deployment "pg-ssl" has env var "DB_POSTGRESDB_SSL_ENABLED" set to "true"
    And the Deployment "pg-ssl" has env var "DB_POSTGRESDB_SSL_CA" set to "/etc/n8n/ssl/ca/ca.crt"
    And the Deployment "pg-ssl" mounts secret "pg-ca" at "/etc/n8n/ssl/ca/ca.crt"

  Scenario: MySQL database wires host and password Secret
    Given a Secret "mysql-creds" exists with key "password" set to "s3cret"
    When I apply a Single "mysql" with MySQL host "mysql.example.com" port 3306 database "n8n" user "n8n" password from secret "mysql-creds" key "password"
    Then the Deployment "mysql" has env var "DB_TYPE" set to "mysqldb"
    And the Deployment "mysql" has env var "DB_MYSQLDB_HOST" set to "mysql.example.com"
    And the Deployment "mysql" sources env var "DB_MYSQLDB_PASSWORD" from secret "mysql-creds" key "password"

  Scenario: spec.persistence provisions a PVC and mounts it at /home/node/.n8n
    When I apply a Single "persisted" with persistence size "1Gi"
    Then a PersistentVolumeClaim named "persisted-data" exists with size "1Gi"
    And the Deployment "persisted" mounts pvc "persisted-data" at "/home/node/.n8n"

  Scenario: Database type mismatch is rejected
    Given a Secret "pg-creds" exists with key "password" set to "s3cret"
    When I apply a Single "bad-db" with database type "postgresdb" and only a MySQL config
    Then the Single "bad-db" never reaches status.ready=true within 15 seconds

  Scenario: Two Instances in the same namespace get independent children
    When I apply a Single "alpha" with image "nginx:alpine"
    And I apply a Single "beta" with image "nginx:alpine"
    Then a Deployment named "alpha" exists in namespace "default" within 60 seconds
    And a Deployment named "beta" exists in namespace "default" within 60 seconds
    And the Deployment "alpha" pods select on label "app.kubernetes.io/instance=alpha"
    And the Deployment "beta" pods select on label "app.kubernetes.io/instance=beta"
    And a Secret named "alpha-encryption-key" exists
    And a Secret named "beta-encryption-key" exists

  Scenario: Child resources carry the recommended app.kubernetes.io labels
    When I apply a Single "labelled" with image "n8nio/n8n:1.70.0"
    Then a Deployment named "labelled" exists in namespace "default" within 60 seconds
    And the Deployment "labelled" has label "app.kubernetes.io/name=n8n"
    And the Deployment "labelled" has label "app.kubernetes.io/instance=labelled"
    And the Deployment "labelled" has label "app.kubernetes.io/managed-by=n8n-rustful-operator"
    And the Deployment "labelled" has label "app.kubernetes.io/part-of=n8n"
    And the Deployment "labelled" has label "app.kubernetes.io/component=workflow-engine"
    And the Deployment "labelled" has label "app.kubernetes.io/version=1.70.0"
    And the Deployment "labelled" has annotation "n8n.slys.dev/operator-version"

  Scenario: Single respects spec.replicas
    When I apply a Single "scaled" with image "nginx:alpine" and replicas 3
    Then a Deployment named "scaled" exists in namespace "default" within 60 seconds
    And the Deployment "scaled" has 3 replicas

  Scenario: Single Service can be exposed as LoadBalancer
    When I apply a Single "lb" with image "nginx:alpine" and service type "LoadBalancer"
    Then the Service "lb" has type "LoadBalancer"

  Scenario: HTTPRoute is provisioned and parent-ref is set when configured
    When I apply a Single "route" with httpRoute gateway "shared-gw" namespace "default" and host "route.example.com"
    Then an HTTPRoute named "route" exists with host "route.example.com" within 60 seconds
    And the HTTPRoute "route" has parent gateway "shared-gw" namespace "default"

  Scenario: httpRoute pins a Gateway listener and adds an HTTP→HTTPS redirect
    When I apply a Single "routed" with httpRoute gateway "gw" namespace "istio-system" section "https" redirect "http" and host "n8n.example.com"
    Then an HTTPRoute named "routed" exists with host "n8n.example.com" within 60 seconds
    And the HTTPRoute "routed" has parent section "https"
    And an HTTPRoute named "routed-redirect" exists with host "n8n.example.com" within 60 seconds
    And the HTTPRoute "routed-redirect" has parent section "http"

  Scenario: Dropping httpRoute from spec removes the HTTPRoute
    Given a Single "rt-drop" exists with httpRoute gateway "shared-gw" and host "rt.example.com"
    When I update the Single "rt-drop" to have no networking
    Then the HTTPRoute "rt-drop" is gone within 60 seconds

  Scenario: Ingress with TLS attaches the named Secret
    When I apply a Single "tls" with ingress class "nginx" host "tls.example.com" and TLS secret "tls-cert"
    Then an Ingress named "tls" exists with host "tls.example.com" within 60 seconds
    And the Ingress "tls" terminates TLS with secret "tls-cert"

  Scenario: Deleting one Single leaves the other untouched
    Given a Single "stayer" exists
    And a Single "leaver" exists
    When I delete the Single "leaver"
    Then the Single "leaver" is gone within 60 seconds
    And a Deployment named "stayer" exists in namespace "default" within 5 seconds
    And a Secret named "stayer-encryption-key" exists

  Scenario: secureCookie sets N8N_SECURE_COOKIE on the Single
    When I apply a Single "cookie" with secureCookie false
    Then the Deployment "cookie" has env var "N8N_SECURE_COOKIE" set to "false"

  Scenario: A Single without secureCookie carries no N8N_SECURE_COOKIE
    When I apply a Single "no-cookie" with image "nginx:alpine"
    Then the Deployment "no-cookie" has no env var "N8N_SECURE_COOKIE"

  Scenario: extraEnv passes a variable straight to the Single container
    When I apply a Single "extra" with extraEnv "N8N_PROXY_HOPS"="1"
    Then the Deployment "extra" has env var "N8N_PROXY_HOPS" set to "1"

  Scenario: extraEnv can source a value from a Secret
    When I apply a Single "envsec" with extraEnv "ANTHROPIC_API_KEY" from secret "n8n-secret" key "ANTHROPIC_API_KEY"
    Then the Deployment "envsec" sources env var "ANTHROPIC_API_KEY" from secret "n8n-secret" key "ANTHROPIC_API_KEY"

  Scenario: extraEnv with both value and valueFrom is rejected
    When I apply a Single "envbad" with extraEnv "FOO" set to both value and valueFrom
    Then the Single "envbad" never reaches status.ready=true within 20 seconds

  Scenario: imagePullSecrets are set on the Single Deployment
    When I apply a Single "private" with imagePullSecret "ghcr-secret"
    Then the Deployment "private" has imagePullSecret "ghcr-secret"

  Scenario: host is wired into the n8n URL env vars
    When I apply a Single "urls" with ingress class "nginx" and host "n8n.example.com"
    Then the Deployment "urls" has env var "N8N_HOST" set to "n8n.example.com"
    And the Deployment "urls" has env var "N8N_PROTOCOL" set to "http"
    And the Deployment "urls" has env var "WEBHOOK_URL" set to "http://n8n.example.com/"
    And the Deployment "urls" has env var "N8N_EDITOR_BASE_URL" set to "http://n8n.example.com"

  Scenario: resources are applied to the Single container
    When I apply a Single "sized" with cpu request "200m" and memory limit "1Gi"
    Then the Deployment "sized" requests cpu "200m" and limits memory "1Gi"
