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

  Scenario: Default Instance has no database env vars
    When I apply an Instance "db-default" with image "nginx:alpine"
    Then a Deployment named "db-default" exists in namespace "default" within 60 seconds
    And the Deployment "db-default" has no env var "DB_TYPE"

  Scenario: Postgres database wires host, user, schema and password from a Secret
    Given a Secret "pg-creds" exists with key "password" set to "s3cret"
    When I apply an Instance "pg" with Postgres host "pg.example.com" port 5432 database "n8n" user "n8n" password from secret "pg-creds" key "password" schema "public" pool size 7
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
    When I apply an Instance "pg-ssl" with Postgres host "pg.example.com" database "n8n" user "n8n" password from secret "pg-creds" key "password" and SSL CA from secret "pg-ca" key "ca.crt"
    Then the Deployment "pg-ssl" has env var "DB_POSTGRESDB_SSL_ENABLED" set to "true"
    And the Deployment "pg-ssl" has env var "DB_POSTGRESDB_SSL_CA" set to "/etc/n8n/ssl/ca/ca.crt"
    And the Deployment "pg-ssl" mounts secret "pg-ca" at "/etc/n8n/ssl/ca/ca.crt"

  Scenario: MySQL database wires host and password Secret
    Given a Secret "mysql-creds" exists with key "password" set to "s3cret"
    When I apply an Instance "mysql" with MySQL host "mysql.example.com" port 3306 database "n8n" user "n8n" password from secret "mysql-creds" key "password"
    Then the Deployment "mysql" has env var "DB_TYPE" set to "mysqldb"
    And the Deployment "mysql" has env var "DB_MYSQLDB_HOST" set to "mysql.example.com"
    And the Deployment "mysql" sources env var "DB_MYSQLDB_PASSWORD" from secret "mysql-creds" key "password"

  Scenario: SQLite with persistence provisions a PVC and mounts it at /home/node/.n8n
    When I apply an Instance "sqlite-pv" with SQLite persistence size "1Gi"
    Then a PersistentVolumeClaim named "sqlite-pv-data" exists with size "1Gi"
    And the Deployment "sqlite-pv" mounts pvc "sqlite-pv-data" at "/home/node/.n8n"

  Scenario: Database type mismatch is rejected
    Given a Secret "pg-creds" exists with key "password" set to "s3cret"
    When I apply an Instance "bad-db" with database type "postgresdb" and only a MySQL config
    Then the Instance "bad-db" never reaches status.ready=true within 15 seconds

  Scenario: Two Instances in the same namespace get independent children
    When I apply an Instance "alpha" with image "nginx:alpine"
    And I apply an Instance "beta" with image "nginx:alpine"
    Then a Deployment named "alpha" exists in namespace "default" within 60 seconds
    And a Deployment named "beta" exists in namespace "default" within 60 seconds
    And the Deployment "alpha" pods select on label "app.kubernetes.io/instance=alpha"
    And the Deployment "beta" pods select on label "app.kubernetes.io/instance=beta"
    And a Secret named "alpha-encryption-key" exists
    And a Secret named "beta-encryption-key" exists

  Scenario: Child resources carry the recommended app.kubernetes.io labels
    When I apply an Instance "labelled" with image "n8nio/n8n:1.70.0"
    Then a Deployment named "labelled" exists in namespace "default" within 60 seconds
    And the Deployment "labelled" has label "app.kubernetes.io/name=n8n"
    And the Deployment "labelled" has label "app.kubernetes.io/instance=labelled"
    And the Deployment "labelled" has label "app.kubernetes.io/managed-by=n8n-rustful-operator"
    And the Deployment "labelled" has label "app.kubernetes.io/part-of=n8n"
    And the Deployment "labelled" has label "app.kubernetes.io/component=workflow-engine"
    And the Deployment "labelled" has label "app.kubernetes.io/version=1.70.0"
    And the Deployment "labelled" has annotation "n8n.slys.dev/operator-version"

  Scenario: Deleting one Instance leaves the other untouched
    Given an Instance "stayer" exists
    And an Instance "leaver" exists
    When I delete the Instance "leaver"
    Then the Instance "leaver" is gone within 60 seconds
    And a Deployment named "stayer" exists in namespace "default" within 5 seconds
    And a Secret named "stayer-encryption-key" exists
