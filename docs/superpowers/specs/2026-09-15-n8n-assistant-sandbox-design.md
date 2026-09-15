# n8n Assistant (self-hosted sandbox) — design

Data: 2026-09-15
Status: zatwierdzony do planowania implementacji

## Problem

`n8n-rustful-operator` nie ma dziś żadnego wsparcia dla n8n Assistant (modułu
`instance-ai`). Asystent wymaga trzech rzeczy: modelu LLM, **obowiązkowego
sandboxa** do wykonywania generowanego kodu, oraz opcjonalnej wyszukiwarki.
Sandbox ma dwa warianty — zarządzana Daytona albo self-hosted `n8n-sandbox`.
Wybrany został **self-hosted**.

Referencyjny stack (`https://raw.githubusercontent.com/n8n-io/n8n/master/docker/get-n8n-compose.yml`,
`compose-version: 1`) składa się z trzech kontenerów:

| serwis | obraz | rola |
| --- | --- | --- |
| `sandbox-certs` | `ghcr.io/n8n-io/n8n-sandbox-service-api:1.2.0` | jednorazowo `bootstrap-mtls.sh --out-dir /tls --api-san sandbox-api --control-san-prefix sandbox-runner`, `NUM_RUNNERS=1` |
| `sandbox-api` | ten sam obraz | HTTP `:8080` (n8n + `/healthz`), gRPC `:9090` (rejestracja runnerów) |
| `sandbox-runner-1` | `ghcr.io/n8n-io/n8n-sandbox-service-runner-dind:1.2.0` | **privileged** Docker-in-Docker, control-gRPC `:9091`, uruchamia sandboxy z `ghcr.io/n8n-io/n8n-sandbox-service-sandbox:latest` |

Dostępność funkcji: self-hosted Community / Registered Community / Business.
Self-hosted Enterprise nie jest wspierany. Moduł `agents` wymaga n8n ≥ 2.32.3.

## Decyzje projektowe

| # | Decyzja | Uzasadnienie |
| --- | --- | --- |
| 1 | Osobny CRD, nie rozszerzenie `Cluster`/`Single` | Sandbox to niezależny podsystem, reużywalny między instancjami |
| 2 | Kind `Assistant`, nie `Sandbox` | Zasób włada całą konfiguracją `instance-ai`; sandbox jest szczegółem implementacji |
| 3 | Certy przez Job → Secret, nie RWX PVC | Brak zależności od StorageClass z RWX; szyfrowanie at-rest; klucz CA nie jest nigdzie utrwalany |
| 4 | Publisher certów jako trzeci bin w obrazie operatora | Brak zewnętrznego obrazu z `kubectl` i dryfu wersji; whitelista plików pod testami |
| 5 | Dokładnie jeden runner | YAGNI; każdy runner to osobny uprzywilejowany DinD. `runners.count` można dodać później bez breaking change |
| 6 | `Assistant` wskazuje na target (`targetRef`), nie odwrotnie | Konfiguracja asystenta w jednym miejscu |
| 7 | Wstrzykiwanie przez `envFrom` + Secret, **nie** przez `env` | `containers[].env` jest w SSA listą atomic — dwa field managery nie mogą jej współdzielić bez flappingu podów |
| 8 | Sandbox w osobnym namespace, sprzątanie przez finalizer | `privileged` zamknięty w jednym namespace; ownerRef nie działa cross-namespace (GC skasowałby obiekty jako sieroty) |
| 9 | Web search: tylko Brave + zewnętrzny SearXNG po URL | Operator nie hostuje niezwiązanego z n8n komponentu; docsy ostrzegają przed rate limitami darmowego SearXNG |

## API: CRD `Assistant`

Grupa `n8n.slys.dev`, wersja `v1`, namespaced, plural `assistants`,
shortname `n8na`, finalizer `assistants.n8n.slys.dev`.

```yaml
apiVersion: n8n.slys.dev/v1
kind: Assistant
metadata:
  name: n8n
  namespace: n8n-cluster
spec:
  targetRef:
    kind: Cluster                 # Cluster | Single — zawsze ten sam namespace co CR
    name: n8n
  model:
    name: anthropic/claude-opus-4-8
    apiKeySecret: { name: n8n-ai, key: ANTHROPIC_API_KEY }
    url: ""                       # opcjonalne: endpoint OpenAI-compatible
  search:
    brave:
      apiKeySecret: { name: n8n-ai, key: BRAVE_API_KEY }
    searxng:
      url: ""                     # opcjonalne: zewnętrzna instancja
  sandbox:
    namespace: n8n-sandbox        # domyślnie: namespace CR-a
    version: "1.2.0"              # tag obu obrazów n8n-sandbox-service-*
    sandboxImage: ghcr.io/n8n-io/n8n-sandbox-service-sandbox:latest
    api:
      resources: {}
      pod: {}                     # istniejący PodConfig operatora
    runner:
      resources:
        requests: { cpu: "1", memory: 2Gi }
        limits:   { memory: 4Gi }
      dockerStorage:
        emptyDir: { sizeLimit: 20Gi }   # albo persistentVolumeClaim
      pod: {}
status:
  ready: false
  certsReady: false
  apiReady: false
  runnerReady: false
  targetSecret: ""
  message: ""
```

Prefiks nazw generowanych obiektów: `<target-ns>-<name>` (dalej `<p>`), żeby
dwa `Assistanty` z różnych namespace'ów mogły dzielić jeden namespace sandboxa.

## Architektura

```
namespace n8n-cluster (restricted)          namespace n8n-sandbox (privileged)
┌──────────────────────────────┐            ┌──────────────────────────────────┐
│ Assistant (CR)               │            │ Job <p>-sandbox-certs            │
│   └─owns─ Secret             │            │   ├─ init: bootstrap-mtls.sh     │
│        <target>-instance-ai  │            │   └─ tlspub → 2 × Secret TLS     │
│                              │            │ SA + Role + RoleBinding          │
│ Deployment n8n (main)        │            │ Deployment <p>-sandbox-api       │
│   envFrom: ↑ (optional)      │──HTTP────▶ │ Service   <p>-sandbox-api :8080  │
└──────────────────────────────┘   :8080    │                          :9090   │
                                            │ Deployment <p>-sandbox-runner-1  │
                                            │   privileged DinD                │
                                            │ Service <p>-sandbox-runner-1     │
                                            │                    :9091 :8080   │
                                            └──────────────────────────────────┘
```

### Własność obiektów

| namespace | obiekt | własność / sprzątanie |
| --- | --- | --- |
| target | `Secret <target>-instance-ai` | ownerRef → `Assistant`, GC |
| sandbox | `Secret <p>-sandbox-secrets` | etykiety + finalizer |
| sandbox | `ServiceAccount`/`Role`/`RoleBinding` `<p>-sandbox-certs` | etykiety + finalizer |
| sandbox | `Job <p>-sandbox-certs` (`ttlSecondsAfterFinished: 3600`) | etykiety + finalizer |
| sandbox | `Secret <p>-sandbox-tls-api`, `<p>-sandbox-tls-runner` | etykiety + finalizer |
| sandbox | `Deployment`/`Service` `<p>-sandbox-api` | etykiety + finalizer |
| sandbox | `Deployment`/`Service` `<p>-sandbox-runner-1` | etykiety + finalizer |

Etykiety identyfikujące: `n8n.slys.dev/assistant: <name>`,
`n8n.slys.dev/assistant-namespace: <ns>`, obok istniejących `common_labels`.

**Krytyczne:** obiekty w namespace sandboxa **nie mogą** mieć `ownerReferences`
wskazujących na CR w innym namespace — Kubernetes uzna je za sieroty i skasuje.

## Przepływ certów mTLS

`bootstrap-mtls.sh` generuje jedno CA i dwa komplety certów w `/tls/api` oraz
`/tls/runner`. Klucze Secreta nie mogą zawierać `/`, więc rozdzielamy na dwa
Secrety montowane pod `/tls`, a ścieżki przestawiamy zmiennymi środowiskowymi
(wszystkie są konfigurowalne).

| Secret | klucze |
| --- | --- |
| `<p>-sandbox-tls-api` | `ca.crt`, `grpc-server.crt`, `grpc-server.key`, `control-grpc-api-client.crt`, `control-grpc-api-client.key` |
| `<p>-sandbox-tls-runner` | `ca.crt`, `grpc-client.crt`, `grpc-client.key`, `control-grpc-server.crt`, `control-grpc-server.key` |

Klucz root CA **nie jest publikowany do żadnego Secreta** — `tlspub` przepisuje
wyłącznie dziesięć plików z whitelisty, reszta ginie z `emptyDir` Joba. Rotacja
nie potrzebuje klucza CA, bo jest regeneracją od zera.

Przebieg:

1. Reconcile sprawdza istnienie obu Secretów TLS. Jeśli istnieją — Job nie
   powstaje. To jedyna ochrona przed przypadkową rotacją CA pod działającymi podami.
2. Jeśli brakuje któregokolwiek — `Job <p>-sandbox-certs`:
   - initContainer `bootstrap`: obraz `n8n-sandbox-service-api:<version>`,
     `runAsUser: 0`, `bootstrap-mtls.sh --out-dir /tls --api-san <p>-sandbox-api
     --control-san-prefix <p>-sandbox-runner`, `NUM_RUNNERS=1`, wynik do `emptyDir`
   - kontener `tlspub`: obraz operatora, czyta `emptyDir`, tworzy oba Secrety
   - `ttlSecondsAfterFinished: 3600` — Job znika sam; `spec.template` Joba jest
     niemutowalny, więc SSA na istniejącym Jobie by się nie powiodło
3. Oba Deploymenty noszą annotację `n8n.slys.dev/tls-revision` z `resourceVersion`
   swojego Secreta TLS. Rotacja: `kubectl delete secret -n <sbx-ns>
   <p>-sandbox-tls-api <p>-sandbox-tls-runner` → nowe CA + automatyczny rollout obu podów.

RBAC Joba: `ServiceAccount <p>-sandbox-certs` + `Role` z `get`/`create` na
`secrets` w namespace sandboxa. Operator ma już `create` na Secretach, więc
kontrola przed privilege escalation tego nie zablokuje.

## Sekrety współdzielone

`Secret <p>-sandbox-secrets`, generowany raz (ta sama logika co
`reconciler/encryption.rs`: istnieje → zostaw, brak → wylosuj):

```
SANDBOX_API_KEYS                      = N8N_SANDBOX_SERVICE_API_KEY
SANDBOX_API_RUNNER_REGISTRATION_TOKEN = SANDBOX_RUNNER_REGISTRATION_TOKEN
SANDBOX_API_RUNNER_API_KEY            = SANDBOX_RUNNER_API_KEYS
```

Każdy sekret występuje w dwóch wariantach nazwy, bo strona API i strona runnera
używają innych nazw zmiennych dla tej samej wartości.

## Workloady

### `<p>-sandbox-api`

`replicas: 1`, `strategy: Recreate` (runner rejestruje się do konkretnej instancji).

```
envFrom: <p>-sandbox-secrets
env:
  SANDBOX_API_GRPC_TLS_CERT_FILE=/tls/grpc-server.crt
  SANDBOX_API_GRPC_TLS_KEY_FILE=/tls/grpc-server.key
  SANDBOX_API_GRPC_TLS_CLIENT_CA_FILE=/tls/ca.crt
  SANDBOX_API_RUNNER_CONTROL_GRPC_TLS_CA_FILE=/tls/ca.crt
  SANDBOX_API_RUNNER_CONTROL_GRPC_TLS_CERT_FILE=/tls/control-grpc-api-client.crt
  SANDBOX_API_RUNNER_CONTROL_GRPC_TLS_KEY_FILE=/tls/control-grpc-api-client.key
  SANDBOX_API_RUNNER_CONTROL_GRPC_TLS_SERVER_NAME=<p>-sandbox-runner-1
volumes: secret <p>-sandbox-tls-api → /tls (ro)
probes:  GET :8080/healthz
Service: 8080 (http), 9090 (grpc), ClusterIP, bez Ingressa
```

### `<p>-sandbox-runner-1`

`replicas: 1`, `strategy: Recreate`, `securityContext.privileged: true`.

```
envFrom: <p>-sandbox-secrets
env:
  SANDBOX_RUNNER_API_GRPC_ADDR=<p>-sandbox-api.<sbx-ns>.svc:9090
  SANDBOX_RUNNER_REGISTRATION_GRPC_SERVER_NAME=<p>-sandbox-api
  SANDBOX_RUNNER_CONTROL_GRPC_LISTEN_ADDR=:9091
  SANDBOX_RUNNER_CONTROL_GRPC_ADVERTISE_ADDR=<p>-sandbox-runner-1.<sbx-ns>.svc:9091
  SANDBOX_RUNNER_HTTP_BASE_URL=http://<p>-sandbox-runner-1.<sbx-ns>.svc:8080
  SANDBOX_RUNNER_ID=runner-1
  SANDBOX_RUNNER_DOCKER_SANDBOX_IMAGE=<spec.sandbox.sandboxImage>
  SANDBOX_RUNNER_REGISTRATION_GRPC_CA_FILE=/tls/ca.crt
  SANDBOX_RUNNER_REGISTRATION_GRPC_CERT_FILE=/tls/grpc-client.crt
  SANDBOX_RUNNER_REGISTRATION_GRPC_KEY_FILE=/tls/grpc-client.key
  SANDBOX_RUNNER_CONTROL_GRPC_TLS_CERT_FILE=/tls/control-grpc-server.crt
  SANDBOX_RUNNER_CONTROL_GRPC_TLS_KEY_FILE=/tls/control-grpc-server.key
  SANDBOX_RUNNER_CONTROL_GRPC_TLS_CLIENT_CA_FILE=/tls/ca.crt
volumes: secret <p>-sandbox-tls-runner → /tls (ro)
         dockerStorage → /var/lib/docker
Service: 9091 (control-grpc), 8080 (http), ClusterIP
```

**SAN kontra adres:** certy są wystawione na krótkie nazwy (`<p>-sandbox-api`,
`<p>-sandbox-runner-1`), a połączenia idą po FQDN cross-namespace. Dlatego
`*_SERVER_NAME` pozostaje krótki, a `*_ADDR` jest pełny — weryfikacja TLS
sprawdza `SERVER_NAME`, nie adres połączenia.

**`/var/lib/docker`:** runner pobiera obraz sandboxa do wewnętrznego Dockera
przy pierwszym użyciu. Na `emptyDir` oznacza to ponowne pobranie po każdym
restarcie poda; `sizeLimit` chroni ephemeral storage node'a. `dockerStorage`
pozwala podstawić PVC, jeśli cache ma być trwały.

## Wstrzykiwanie do `Cluster`/`Single`

`Secret <target>-instance-ai` w namespace targetu:

```
N8N_ENABLED_MODULES=instance-ai
N8N_INSTANCE_AI_MODEL=<model.name>
N8N_INSTANCE_AI_MODEL_API_KEY=<skopiowany z model.apiKeySecret>
N8N_INSTANCE_AI_MODEL_URL=<model.url, jeśli ustawiony>
N8N_INSTANCE_AI_SANDBOX_ENABLED=true
N8N_INSTANCE_AI_SANDBOX_PROVIDER=n8n-sandbox
N8N_INSTANCE_AI_SANDBOX_IMAGE=<sandbox.sandboxImage>
N8N_INSTANCE_AI_SANDBOX_API_URL=http://<p>-sandbox-api.<sbx-ns>.svc:8080
N8N_SANDBOX_SERVICE_URL=http://<p>-sandbox-api.<sbx-ns>.svc:8080
N8N_SANDBOX_SERVICE_API_KEY=<= SANDBOX_API_KEYS>
INSTANCE_AI_BRAVE_SEARCH_API_KEY=<skopiowany, jeśli ustawiony>
N8N_INSTANCE_AI_SEARXNG_URL=<search.searxng.url, jeśli ustawiony>
```

`INSTANCE_AI_BRAVE_SEARCH_API_KEY` celowo **nie** ma prefiksu `N8N_` — tak
wymagają docsy. Brave ma pierwszeństwo nad SearXNG, gdy ustawione oba.

Kompromis: klucze modelu i Brave są **kopiowane** ze źródłowego Secreta.
Materiał sekretny istnieje w dwóch miejscach. Alternatywa (drugi `envFrom` na
Secret użytkownika) wymagałaby, żeby `Cluster` znał jego nazwę — czyli powrotu
do konfiguracji rozbitej na dwa CR-y. Rotacja źródła propaguje się przy
najbliższym reconcile (≤ 5 min).

Cykl życia domyka się sam: usunięcie `Assistanta` → GC kasuje Secret → `Cluster`
przy reconcile widzi brak, zmienia annotację z rewizją → rollout bez zmiennych
asystenta.

## Zmiany w istniejącym kodzie

| plik | zmiana |
| --- | --- |
| `src/builders/deployment.rs`, `src/builders/cluster_deployment.rs` | stałe `envFrom: [{secretRef: {name: <cr>-instance-ai, optional: true}}]` na kontenerze n8n — rola `main` i `Single`, nie workery ani webhooki |
| `src/reconciler/cluster_main.rs`, `src/reconciler/single_children.rs` | `GET` Secreta `<cr>-instance-ai`, stempel `pod.annotations["n8n.slys.dev/instance-ai-revision"]` (`resourceVersion` albo `"none"`) |
| `src/reconciler/run.rs` | trzeci `Controller` w `join` |
| `src/error.rs` | wariant `IllegalAssistant(String)` |
| `src/lib.rs` | eksport nowych typów |
| `src/crdgen.rs` | `Assistant::crd()` |
| `Cargo.toml` | bin `tlspub` |
| `yaml/install.yaml` | RBAC: `assistants` + `/status` + `/finalizers`, `jobs`, `serviceaccounts`, `roles`, `rolebindings` |

`Single` i `Cluster` nie zyskują żadnego nowego pola w specu ani wiedzy o
`Assistancie`.

## Nowe moduły

```
src/spec/assistant.rs              Assistant/AssistantSpec/AssistantStatus, ASSISTANT_FINALIZER
src/reconciler/assistant.rs        watcher_config, reconcile, error_policy, cleanup
src/reconciler/assistant_apply.rs  walidacja → sekrety → certy → workloady → Secret targetu → status
src/reconciler/assistant_certs.rs  detekcja Secretów TLS i budowa Joba
src/reconciler/assistant_validate.rs
src/reconciler/assistant_status.rs patch_status przez SSA
src/builders/sandbox.rs            build_certs_job, build_sandbox_api, build_sandbox_runner, build_sandbox_rbac
src/env/instance_ai.rs             zawartość Secreta targetu
src/bin/tlspub.rs                  publisher certów
```

## Walidacja

- `targetRef.kind` ∈ `{Cluster, Single}`, `name` niepuste
- `model.name` w formacie `provider/model`, provider ∈ `{anthropic, openai, openrouter}`;
  przy ustawionym `model.url` walidowana tylko niepustość
- `model.apiKeySecret` wymagany, chyba że `model.url` jest ustawiony
- `sandbox.namespace`, `sandbox.version`, `sandbox.sandboxImage` — niepuste,
  `namespace` jako poprawna etykieta DNS-1123
- **limit długości nazw:** najdłuższy generowany obiekt to Service
  `<target-ns>-<name>-sandbox-runner-1`; limit nazwy Service to 63 znaki.
  Walidacja odrzuca CR z góry, zamiast tworzyć część stacka i wywalać się na
  ostatnim obiekcie. SAN-y muszą być identyczne z nazwami Service'ów.

Błędy jako `Error::IllegalAssistant(String)`, z `metric_label()` jak reszta.

## Status i diagnostyka

Odrzucenie przez PSA nie wywala Deploymentu — Deployment i ReplicaSet powstają,
pody nie. Kubernetes wystawia wtedy warunek `ReplicaFailure` z dokładnym
powodem. Reconcile czyta `.status.conditions` obu Deploymentów i przepisuje
powód do `status.message` oraz do eventu, żeby „namespace bez `enforce:
privileged`" było widoczne w `kubectl get assistant`.

Każde pole statusu musi znaleźć się w bloku `Patch::Apply` w
`assistant_status.rs` — SSA usuwa pola pominięte w patchu.

## Testy

Testy przed implementacją (TDD).

Jednostkowe (`just test-unit`):

- kształt Joba certów, obu Deploymentów, Service'ów i RBAC
- zawartość `Secret <target>-instance-ai` dla wszystkich kombinacji
  `model.url` / Brave / SearXNG
- walidacja: wszystkie reguły powyżej, w tym limit 63 znaków
- `tlspub`: whitelista plików — test, że klucz CA **nigdy** nie trafia do Secreta
- `envFrom` i annotacja rewizji na Deploymentach `Cluster`/`Single`

BDD (`features/assistant.feature`, `kind`, workflow `e2e.yml`):

utworzenie `Assistanta` → Job kończy się sukcesem → oba Secrety TLS istnieją →
`sandbox-api` gotowy → runner zarejestrowany w logach API → Secret targetu ma
oczekiwane klucze → usunięcie CR-a czyści namespace sandboxa. Scenariusz jest
ciężki (DinD), więc nie wchodzi do `just test-unit`.

Weryfikacja ręczna wg docsów: `curl <api>:8080/healthz` → `{"status":"ok"}`,
`kubectl logs <api> | grep -i runner`.

## Wymagania po stronie klastra

Nie realizuje ich operator; wchodzą do dokumentacji wdrożeniowej.

1. Namespace sandboxa z `pod-security.kubernetes.io/enforce: privileged`.
   Namespace targetu może zostać `restricted`.
2. Ruch cross-namespace z n8n do `<p>-sandbox-api:8080`. Przy NetworkPolicy
   (Cilium) lub `PeerAuthentication: STRICT` (Istio) trzeba go jawnie dopuścić.
3. Zapas zasobów: docsy podają minimum 4 GB RAM i 2 vCPU na stack sandboxa.
4. Żaden port sandboxa nie może być wystawiony publicznie — runner jest
   uprzywilejowanym DinD, równoważnym rootowi na node'zie.

## Świadomie poza zakresem

- Wiele runnerów (`runners.count`) — dodawalne bez breaking change
- Hostowanie SearXNG przez operator
- Provider `daytona`
- Automatyczne odnawianie certów (rotacja jest ręczna: skasowanie Secretów TLS)
- Moduł `agents`
