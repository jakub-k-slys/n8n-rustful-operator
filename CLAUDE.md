# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

Kubernetes operator written in Rust on top of `kube-rs` (v3). It reconciles two custom
resources in the group `n8n.slys.dev` (v1, namespaced) into running n8n deployments:

- `Single` (kind `Single`, shortname `n8n`, plural `singles`) — one standalone n8n process:
  a `Deployment` + `Service`, plus an optional encryption-key `Secret`, `PersistentVolumeClaim`,
  and `Ingress`/`HTTPRoute`.
- `Cluster` (kind `Cluster`, shortname `n8nc`, plural `clusters`) — n8n in **queue mode**:
  separate `main`, `worker` and (optional) `webhook` roles backed by a shared database and
  Redis, with optional HPA-driven worker autoscaling.

Every child object is created with server-side apply (field manager `n8n-rustful-operator`),
owned by the parent CR, and the operator patches the resource's `.status` back. Scaffold is
modelled on `kube-rs/controller-rs`.

## Commands

A `justfile` wraps the common flows. Plain `cargo`/`kubectl` work too.

```sh
just generate         # cargo run --bin crdgen > yaml/crd.yaml
just install-crd      # generate + kubectl apply -f yaml/crd.yaml
just run              # RUST_LOG=info,kube=debug,n8n_rustful_operator=debug cargo run
just run-telemetry    # same, but with --features=telemetry and OPENTELEMETRY_ENDPOINT_URL
just test-unit        # cargo test
just fmt              # cargo fmt
just compile          # static musl build inside clux/muslrust:stable (for Dockerfile)
just build-base       # compile + docker build -t jslys/n8n-rustful-operator:local .
just build-otel       # same, with the telemetry feature
```

Run a single test: `cargo test <name>` (or `cargo test --lib <name> -- --ignored` for ignored ones).
The BDD suite is a separate test target — see **Testing** below.

Both CRDs must exist in the cluster before `just run` — the controller calls `list` on
`Single` and `Cluster` at startup and `exit(1)`s if either API is not registered. Always
`just generate && just install-crd` after editing any spec/status type.

## Architecture

Single crate, two binaries, one library. The reconciler, CRD spec, child-object builders,
env wiring and metrics each live in their own module tree (the controller was originally one
`controller.rs` file; it has since been split per-concern).

### Top level

- `src/main.rs` — `actix-web` server on `:8080` (`GET /`, `/health`, `/metrics`,
  `PUT /log-level`). Runs the controller and the HTTP server concurrently via `tokio::join!`.
- `src/lib.rs` — declares the modules and re-exports the public surface (`Single`, `Cluster`,
  their spec/status types, `State`, `Context`, `run`, `Error`, `Metrics`).
- `src/error.rs` — the `Error` enum used as the reconciler's error type, plus `Result`.
  Variants: `SerializationError`, `KubeError`, `FinalizerError(Box<...>)`, `IllegalSingle`,
  `ConflictingNetworking`, `IllegalDatabase(String)`, `IllegalCluster(String)`.
  `metric_label()` is the lowercased Debug, used as the Prometheus `error` label.
- `src/state.rs` — `State` (shared between web server and controller) and `Context`
  (per-reconcile: `Client`, event `Recorder`, `Diagnostics`, `Metrics`). `to_context`
  builds the `Context`.
- `src/labels.rs` — `selector_labels` (the immutable `app.kubernetes.io/{name,instance}`
  used as Deployment/Service selectors), `common_labels` (full recommended set incl.
  `managed-by`, `part-of`, `component`, `version`), and `common_annotations`
  (`n8n.slys.dev/operator-version` from `CARGO_PKG_VERSION`).
- `src/telemetry.rs` — `tracing` subscriber with an `EnvFilter` reload handle (lets
  `PUT /log-level` rewrite the filter at runtime). OTLP/gRPC tracer is gated behind the
  `telemetry` feature.
- `src/crdgen.rs` — second binary; prints the CRD YAML for both `Single::crd()` and
  `Cluster::crd()`.

### `src/spec/` — the CRD types

One module per concern, all re-exported from `spec::*`:

- `single.rs` — `Single` / `SingleSpec` / `SingleStatus`, `SINGLE_FINALIZER`, `default_image`.
- `cluster.rs` — `Cluster` / `ClusterSpec` / `ClusterStatus`, `CLUSTER_FINALIZER`.
- `roles.rs` — `MainConfig`, `WorkerConfig`, `WebhookConfig`, `Autoscaling` (the per-role
  config for a `Cluster`).
- `database.rs` — `DatabaseSpec` (`type` ∈ `sqlite`/`postgresdb`/`mysqldb`/`mariadb`),
  `PostgresConfig`, `MysqlConfig`, `SqliteConfig`, `DatabaseSsl`.
- `redis.rs` — `RedisConfig` (queue broker for `Cluster`).
- `networking.rs` — `NetworkingSpec` (`ingress` **or** `httpRoute`, mutually exclusive),
  `IngressConfig`, `HttpRouteConfig`, `GatewayRef`.
- `common.rs` — shared building blocks: `SecretKeyRef`, `EncryptionKeySpec`, `ServiceConfig`,
  `PersistenceConfig`.

The `#[kube(...)]` attribute sets `plural` explicitly (`singles`, `clusters`) — without it,
kube-derive would mis-pluralize. The finalizer strings mirror the CRD names
(`singles.n8n.slys.dev`, `clusters.n8n.slys.dev`).

### `src/reconciler/` — the control loop

- `run.rs` (`run`) — creates the client, verifies both CRDs are queryable, then starts two
  `Controller`s (`Single` and `Cluster`) joined with `futures::future::join`.
- `single.rs` / `cluster.rs` — each defines `watcher_config`, `reconcile` (wraps the apply
  in `kube::runtime::finalizer`, so cleanup runs on delete), `error_policy`, and `cleanup`
  (publishes a `DeleteRequested` event).
- `single_apply.rs` / `cluster_apply.rs` (`apply`) — the per-CR logic: validate, resolve the
  encryption Secret, apply children, emit an `Applied` event, patch `.status`, requeue in 5m.
- `ctx.rs` — `ApplyCtx` (the `client`/`ns`/`owner`/`patch` bundle threaded through every
  child apply, with a generic `.api::<K>()` helper) and `Bundle` (the env/volumes/mounts
  payload shared across cluster roles).
- `owner.rs` — `single_owner` / `cluster_owner` build the `OwnerReference` for SSA.
- `encryption.rs` — `resolve_encryption_secret`: returns the user's `secretRef` if given,
  otherwise creates a random 32-byte hex `Secret` named `<name>-encryption-key` (owned by
  the CR) if it doesn't already exist.
- `validate.rs` — `validate_database` (type/sub-block consistency) and `validate_cluster`
  (also rejects `sqlite`, which queue mode cannot use).
- `single_validate.rs` — `validate_single`: rejects name `"illegal"` (test hook →
  `Error::IllegalSingle`), conflicting `ingress`+`httpRoute`, and bad database config.
- `single_children.rs` — applies the `Single`'s PVC (if any), `Deployment`, `Service`, and
  networking.
- `cluster_main.rs`, `cluster_worker.rs`, `cluster_webhook.rs`, `cluster_main_volumes.rs` —
  per-role apply for a `Cluster`. Workers get a `Deployment` (and an HPA when autoscaling is
  set) but no `Service`.
- `networking.rs` — `reconcile_role_networking`: provisions/garbage-collects the `Ingress`
  or `HTTPRoute` for a role, including removing it when the spec drops `networking`.
- `single_status.rs` / `cluster_status.rs` — `patch_status` via SSA.

### `src/builders/` — pure object constructors

`build_deployment`, `build_cluster_deployment`, `build_service`, `build_ingress`,
`build_http_route`, `build_hpa`, `build_data_pvc`, and `volumes` (DB/SSL volume + mount
wiring). These take spec + owner and return the JSON/typed object to apply — no I/O.

### `src/env/` — n8n environment variables

`env_str` / `env_secret` helpers, plus `database.rs` (maps `DatabaseSpec` to `DB_*` env and
SSL cert mounts) and `redis.rs` (`build_cluster_common_env` — `EXECUTIONS_MODE=queue`,
`QUEUE_BULL_REDIS_*`, encryption key, etc., shared by all cluster roles).

### `src/metrics/` — Prometheus

Registry with prefix `n8n_operator_reconcile` (`mod.rs`). `reconcile.rs` defines
`ReconcileMetrics` (`runs`, `failures`, `duration`) and `ReconcileMeasurer`, which records
duration via `Drop`. `labels.rs` holds `ErrorLabels` (`instance` = `name_any()` of the
offending CR, `error` = `Error::metric_label()`) and `TraceLabel` (exemplar trace id). The
`"illegal"` instance name triggers `Error::IllegalSingle` and is used in tests to assert the
failure path.

## Conventions

- Commit messages follow [Conventional Commits](https://www.conventionalcommits.org/) /
  semver: `feat:`, `fix:`, `chore:`, `refactor:`, `docs:`, `test:`, `build:`, `ci:`.
  Breaking changes use `feat!:` / `fix!:` or a `BREAKING CHANGE:` footer. The prefix drives
  the version bump (`feat` → minor, `fix` → patch, `!`/`BREAKING CHANGE` → major).
- `rustfmt.toml` is restricted to stable options. Format with `cargo fmt` (or `just fmt`)
  on the stable toolchain — no nightly required.
- All k8s writes from this operator use server-side apply with field manager
  `n8n-rustful-operator`. Keep that consistent — switching managers mid-flight will leave
  orphaned managed fields. Selector labels (`selector_labels`) are immutable; never change them.
- Reconciler errors must round-trip through `Error::FinalizerError(Box<...>)`. The boxing
  exists to break a cycle in the type — don't try to flatten it.
- Modules are kept small and per-concern. New child objects → a builder in `src/builders/`
  plus an apply step in the relevant `reconciler/*` module; new env → `src/env/`.

## Adding fields to the CRD

1. Edit the relevant type under `src/spec/` (`SingleSpec`/`SingleStatus`, `ClusterSpec`/
   `ClusterStatus`, or a shared block like `DatabaseSpec`).
2. `just generate` to refresh `yaml/crd.yaml`.
3. `just install-crd` to update the live cluster.
4. If you added a **status** field, also update the `Patch::Apply` block in the relevant
   `*_status.rs` — server-side apply drops fields not present in the patch.
5. If the field affects child objects, wire it through the matching builder in `src/builders/`
   (and `src/env/` for new env vars), and add validation in `reconciler/validate.rs` or
   `single_validate.rs` if it can be mis-configured.

## Telemetry feature

`#[cfg(feature = "telemetry")]` gates OTLP wiring in `telemetry.rs`. The `unused_imports`
allow at the top of that file keeps the non-telemetry build clean. The web server is
identical in both feature configurations.

## Testing

- `just test-unit` (`cargo test`) — unit tests.
- `features/` — Cucumber BDD suite. `features/cucumber.rs` is registered as a `[[test]]`
  target in `Cargo.toml`; `single.feature` and `cluster.feature` exercise the operator
  against a `kind` cluster (driven in CI by `.github/workflows/e2e.yml`).

## Deploying

`yaml/install.yaml` bundles the Namespace, ServiceAccount, RBAC (ClusterRole/Binding) and the
operator Deployment (image `ghcr.io/jakub-k-slys/n8n-rustful-operator`, `__IMAGE_TAG__`
placeholder). The ClusterRole grants the operator the verbs it needs on `singles`/`clusters`
(+ `/status`, `/finalizers`), Deployments, Services, Secrets, PVCs, Ingresses, HTTPRoutes,
HPAs and Events. Apply the CRDs (`just install-crd`) before `install.yaml`.
