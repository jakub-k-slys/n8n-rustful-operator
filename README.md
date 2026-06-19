# n8n-rustful-operator

Kubernetes operator (in Rust, on [`kube-rs`](https://github.com/kube-rs/kube)) that
reconciles custom resources into running [n8n](https://n8n.io) deployments. Scaffold is
modelled on [`kube-rs/controller-rs`](https://github.com/kube-rs/controller-rs).

It manages two custom resources in the `n8n.slys.dev/v1` group:

- **`Single`** — one standalone n8n process (Deployment + Service, plus optional
  database, persistence, networking and an encryption-key Secret).
- **`Cluster`** — n8n in [queue mode](https://docs.n8n.io/hosting/scaling/queue-mode/):
  separate `main`, `worker` and (optional) `webhook` roles backed by a shared database
  and Redis, with optional HPA-driven worker autoscaling.

All child objects are created with server-side apply (field manager
`n8n-rustful-operator`), owned by the parent CR (so they are garbage-collected on delete),
and labelled with the recommended `app.kubernetes.io/*` set.

## Custom resources

### `Single` (`kind: Single`, shortname `n8n`, plural `singles`)

A single n8n deployment. The reconciler produces a `Deployment` + `Service` and reports
back through `.status` (`ready`, `replicas`, `url`, `encryptionKeySecret`).

| Field           | Description                                                                 |
| --------------- | --------------------------------------------------------------------------- |
| `image`         | Container image (default `n8nio/n8n:latest`).                                |
| `replicas`      | Deployment replicas (default `1`).                                           |
| `host`          | External hostname. Required when `networking` is set.                        |
| `service`       | `type`: `ClusterIP` (default), `NodePort`, or `LoadBalancer`.               |
| `networking`    | Provision an `ingress` **or** an `httpRoute` (mutually exclusive).           |
| `encryptionKey` | `secretRef` to an existing Secret; omit to auto-generate `<name>-encryption-key`. |
| `database`      | `sqlite` (default), `postgresdb`, `mysqldb`/`mariadb` — see below.          |
| `persistence`   | PVC mounted at `/home/node/.n8n` (binary data and the sqlite file).         |

### `Cluster` (`kind: Cluster`, shortname `n8nc`, plural `clusters`)

n8n in queue mode. Requires a shared `database` (sqlite is **rejected**) and `redis`.
Produces a `Deployment` + `Service` for `main` and `webhooks`, and a `Deployment` for
`workers` (workers get no Service). The encryption Secret is shared by all roles.
Status reports `mainReplicas`, `workerReplicas`, `webhookReplicas`.

| Field           | Description                                                              |
| --------------- | ---------------------------------------------------------------------- |
| `image`         | Cascading default image; each role can override.                       |
| `encryptionKey` | Shared across roles; auto-generated if omitted.                        |
| `database`      | **Required** shared DB (`postgresdb` / `mysqldb` / `mariadb`).         |
| `redis`         | **Required** queue broker (host, port, db, auth Secrets, tls, prefix). |
| `main`          | `replicas`, image override, `host`, `service`, `networking`, `persistence`. |
| `workers`       | `replicas`, image, `concurrency`, optional `autoscaling` (HPA).        |
| `webhooks`      | Optional dedicated webhook role (`replicas`, `host`, `service`, …).    |

When `workers.autoscaling` (`minReplicas`/`maxReplicas`/`targetCPUUtilizationPercentage`)
is set, the operator provisions a `HorizontalPodAutoscaler` and stops managing
`spec.replicas` on the worker Deployment. Removing the block deletes the HPA.

### Database backends

`database.type` selects the backend and which sub-block is required:

- `sqlite` (default) — optional `poolSize`, `vacuumOnStartup`, `database` path.
- `postgresdb` — requires `postgres` (host, database, user, `passwordSecret`, optional
  `port`, `schema`, `ssl`, `poolSize`, `connectionTimeoutMs`).
- `mysqldb` / `mariadb` — requires `mysql` (host, database, user, `passwordSecret`, …).

Passwords and TLS material are referenced from Secrets, never inlined. Setting a sub-block
that does not match `type` is rejected, as is `postgresdb`/`mysqldb` without its sub-block.

## Run

The CRDs must exist in the cluster before the operator starts — it lists both on startup
and `exit(1)`s if either API is not registered.

```sh
just generate         # write yaml/crd.yaml from the Rust types
just install-crd      # generate + apply CRDs to the current kube context
just run              # run the operator against the current kube context

kubectl apply -f yaml/single-sample.yaml    # example Single
kubectl apply -f yaml/cluster-sample.yaml   # example Cluster
```

`kubectl get n8n` lists `Single`s; `kubectl get n8nc` lists `Cluster`s.

### Deploy into the cluster

`yaml/install.yaml` bundles the Namespace, ServiceAccount, RBAC and the operator
Deployment (image `ghcr.io/jakub-k-slys/n8n-rustful-operator`). Apply the CRDs first:

```sh
just install-crd
kubectl apply -f yaml/install.yaml   # replace __IMAGE_TAG__ with a released tag
```

## Endpoints

The operator exposes HTTP on `:8080`:

- `GET /` — diagnostics
- `GET /health` — liveness / readiness
- `GET /metrics` — Prometheus (`n8n_operator_reconcile` prefix)
- `PUT /log-level` — runtime log filter (`{"filter": "info,kube=debug"}`)

## Telemetry

Build with `--features=telemetry` (`just run-telemetry` / `just build-otel`) and set
`OPENTELEMETRY_ENDPOINT_URL` to ship traces via OTLP/gRPC.

## Tests

- `just test-unit` — unit tests (`cargo test`).
- `features/*.feature` — Cucumber BDD suite (`features/cucumber.rs`) run against a `kind`
  cluster in CI (`.github/workflows/e2e.yml`).

## Development

See [CLAUDE.md](CLAUDE.md) for the source layout and conventions. In short: format with
`cargo +nightly fmt` (`just fmt`), follow Conventional Commits, and after editing a
spec/status type run `just generate && just install-crd` to refresh the CRDs.
