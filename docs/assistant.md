# Assistant

`Assistant` configures the n8n Assistant (`instance-ai`) module for an
existing `Cluster` or `Single` in the same namespace, and manages the
self-hosted sandbox stack that module requires (`sandbox-api` +
`sandbox-runner`, both from the `ghcr.io/n8n-io/n8n-sandbox-service-api`
image, plus the private sandbox image
`ghcr.io/n8n-io/n8n-sandbox-service-sandbox` started per execution).

## Example CR

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

`targetRef` must point at a CR in the same namespace as the `Assistant` — the
generated Secret carrying the `instance-ai` configuration is owned by this
CR, and an owner reference cannot cross a namespace boundary.

## Cluster requirements

- **The sandbox namespace must already exist** and carry the label
  `pod-security.kubernetes.io/enforce: privileged` — `sandbox-runner` runs a
  privileged Docker-in-Docker (DinD) container, which the `restricted`/
  `baseline` Pod Security Standard rejects. The operator refuses to create
  the stack if the namespace doesn't exist, rather than creating it itself.
- If `sandbox.namespace` differs from the `Assistant`'s own namespace,
  **cross-namespace network traffic must be allowed** — a default deny-all
  `NetworkPolicy`, or a mesh (e.g. Istio) in STRICT mTLS mode, will block
  n8n → `sandbox-api` (HTTP `:8080`, gRPC `:9090`) and `sandbox-api` →
  `sandbox-runner` (control-gRPC `:9091`).
- Reserve at least **4 GB RAM and 2 vCPU** for the sandbox stack itself
  (`sandbox-api` + `sandbox-runner` + the inner Docker daemon) — DinD, and
  the model-generated code it executes, can be surprisingly hungry.
- **No sandbox port should be exposed publicly** (Ingress, LoadBalancer,
  HTTPRoute) — `sandbox-api`/`sandbox-runner` have no authorization of their
  own beyond the shared API keys and the mTLS between them; only n8n, via
  `ClusterIP` in the same cluster, should ever reach them.

## mTLS certificate rotation

The `sandbox-api`/`sandbox-runner` stack talks over mTLS generated once by
the `bootstrap-mtls.sh` bootstrap `Job`. The Job is not rerun as long as both
TLS Secrets exist — and `bootstrap-mtls.sh` writes the CA's private key to
`/tls/ca.key`, outside the two subdirectories `tlspub` copies files from into
the Secrets, so the CA key never reaches the cluster and can't be recovered.
Rotation therefore means deleting both TLS Secrets — once the old bootstrap
Job has finished (succeeded or failed), the operator deletes it and recreates
it with a fresh CA and certificates on the next reconcile, without waiting
out its `ttlSecondsAfterFinished`:

```sh
kubectl delete secret -n <sbx-ns> <prefix>-sandbox-tls-api <prefix>-sandbox-tls-runner
```

where `<prefix>` is `<Assistant's namespace>-<Assistant's name>` (see
`status.sandboxNamespace`/the sandbox object naming). After the Secrets are
deleted, `sandbox-api` and `sandbox-runner` keep running on their old
certificates until their Pods restart — restart both Deployments once the
new certificates appear.

## Verification

Check `sandbox-api`'s health:

```sh
kubectl exec -n <sbx-ns> deploy/<prefix>-sandbox-api -- \
  curl -s <api>:8080/healthz
# {"status":"ok"}
```

Confirm the runner has registered by checking `sandbox-api`'s logs:

```sh
kubectl logs -n <sbx-ns> deploy/<prefix>-sandbox-api | grep -i runner
```

The CR's own status (`ready`, `certsReady`, `apiReady`, `runnerReady`,
`targetSecret`, `sandboxNamespace`) is visible via `kubectl get assistant -n
<namespace> <name> -o yaml`.
