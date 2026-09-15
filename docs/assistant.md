# Assistant

`Assistant` konfiguruje moduł n8n Assistant (`instance-ai`) dla istniejącego
`Cluster`-a lub `Single`-a w tym samym namespace i zarządza wymaganym przez
ten moduł, self-hostowanym stackiem sandboksa (`sandbox-api` +
`sandbox-runner`, oba z obrazu `ghcr.io/n8n-io/n8n-sandbox-service-api`, plus
prywatny obraz sandboksa `ghcr.io/n8n-io/n8n-sandbox-service-sandbox`
uruchamiany per wykonanie).

## Przykładowy CR

```yaml
apiVersion: n8n.slys.dev/v1
kind: Assistant
metadata:
  name: n8n
  namespace: n8n-cluster
spec:
  targetRef:
    kind: Cluster
    name: n8n
  model:
    name: anthropic/claude-opus-4-8
    apiKeySecret:
      name: n8n-ai
      key: ANTHROPIC_API_KEY
  search:
    brave:
      apiKeySecret:
        name: n8n-ai
        key: BRAVE_API_KEY
  sandbox:
    namespace: n8n-sandbox
    version: "1.2.0"
    runner:
      dockerStorage:
        size: 20Gi
```

`targetRef` musi wskazywać na CR w tym samym namespace co `Assistant` —
wygenerowany Secret z konfiguracją `instance-ai` jest właścicielem (owner
reference) tego CR-a, a referencje właściciela nie mogą przekraczać granic
namespace'ów.

## Wymagania wobec klastra

- **Namespace sandboksa musi istnieć z góry** i mieć etykietę
  `pod-security.kubernetes.io/enforce: privileged` — `sandbox-runner`
  uruchamia uprzywilejowany kontener Docker-in-Docker (DinD), którego Pod
  Security Standard `restricted`/`baseline` odrzuci. Operator odmawia
  utworzenia stacku, jeśli namespace nie istnieje, zamiast tworzyć go
  samodzielnie.
- Jeśli `sandbox.namespace` różni się od namespace'u `Assistant`-a, **ruch
  sieciowy między namespace'ami musi być dopuszczony** — domyślny
  `NetworkPolicy` typu deny-all albo mesh (np. Istio) w trybie STRICT mTLS
  zablokuje połączenia n8n → `sandbox-api` (HTTP `:8080`, gRPC `:9090`) oraz
  `sandbox-api` → `sandbox-runner` (control-gRPC `:9091`).
- Rezerwuj co najmniej **4 GB RAM i 2 vCPU** na sam stack sandboksa
  (`sandbox-api` + `sandbox-runner` + wewnętrzny Docker) — DinD i wykonywany
  w nim kod generowany przez model potrafią być zaskakująco żarłoczne.
- **Żaden port sandboksa nie powinien być wystawiony publicznie** (Ingress,
  LoadBalancer, HTTPRoute) — `sandbox-api`/`sandbox-runner` nie mają własnej
  autoryzacji poza współdzielonymi kluczami API i mTLS między sobą; dostęp
  ma mieć wyłącznie n8n przez `ClusterIP` w tym samym klastrze.

## Rotacja certyfikatów mTLS

Stack `sandbox-api`/`sandbox-runner` komunikuje się po mTLS wygenerowanym
jednorazowo przez Job `bootstrap-mtls.sh`. Job nie jest uruchamiany ponownie,
dopóki oba Secrety TLS istnieją — a `bootstrap-mtls.sh` zapisuje klucz
prywatny CA do `/tls/ca.key`, poza dwoma podkatalogami, z których `tlspub`
kopiuje pliki do Secretów, więc klucz CA nigdy nie trafia do klastra i nie da
się go odzyskać. Rotacja polega więc na usunięciu obu Secretów TLS — operator
przy najbliższym reconcile odtworzy Job bootstrapujący nowe CA i certyfikaty:

```sh
kubectl delete secret -n <sbx-ns> <prefix>-sandbox-tls-api <prefix>-sandbox-tls-runner
```

gdzie `<prefix>` to `<namespace Assistant-a>-<nazwa Assistant-a>` (patrz
`status.sandboxNamespace`/nazewnictwo obiektów sandboksa). Po usunięciu
Secretów `sandbox-api` i `sandbox-runner` będą działać na starych
certyfikatach do restartu Podów — zrestartuj oba Deploymenty, gdy nowe
certyfikaty się pojawią.

## Weryfikacja

Sprawdź zdrowie `sandbox-api`:

```sh
kubectl exec -n <sbx-ns> deploy/<prefix>-sandbox-api -- \
  curl -s <api>:8080/healthz
# {"status":"ok"}
```

Potwierdź, że runner się zarejestrował, przeglądając logi `sandbox-api`:

```sh
kubectl logs -n <sbx-ns> deploy/<prefix>-sandbox-api | grep -i runner
```

Status samego CR-a (`ready`, `certsReady`, `apiReady`, `runnerReady`,
`targetSecret`, `sandboxNamespace`) pokazuje `kubectl get assistant -n
<namespace> <name> -o yaml`.
