# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

Kubernetes operator written in Rust on top of `kube-rs` (v3). Reconciles `Instance`
(group `n8n.slys.dev`, v1, shortname `n8n`) custom resources into running n8n deployments —
each CR produces a `Deployment` + `Service` via server-side apply, and the operator patches
the resource's `.status` back. Scaffold is modelled on `kube-rs/controller-rs`.

## Commands

A `justfile` wraps the common flows. Plain `cargo`/`kubectl` work too.

```sh
just generate         # cargo run --bin crdgen > yaml/crd.yaml
just install-crd      # generate + kubectl apply -f yaml/crd.yaml
just run              # RUST_LOG=info,kube=debug,n8n_rustful_operator=debug cargo run
just run-telemetry    # same, but with --features=telemetry and OPENTELEMETRY_ENDPOINT_URL
just test-unit        # cargo test
just fmt              # cargo +nightly fmt   (rustfmt.toml uses nightly-only options)
just compile          # static musl build inside clux/muslrust:stable (for Dockerfile)
just build-base       # compile + docker build -t jslys/n8n-rustful-operator:local .
```

Run a single test: `cargo test <name>` (or `cargo test --lib <name> -- --ignored` for ignored ones).

The CRD must exist in the cluster before `just run` — the controller calls `list` on startup
and `exit(1)`s if the API is not registered. Always `just generate && just install-crd`
after editing the spec/status types.

## Architecture

Single crate, two binaries, one library — layout from `controller-rs`:

- `src/main.rs` — `actix-web` server on `:8080` (`/`, `/health`, `/metrics`,
  `PUT /log-level`). Runs the controller and the HTTP server concurrently via `tokio::join!`.
- `src/lib.rs` — re-exports `controller::*` (so `Instance`, `State`, `run` are crate-root),
  defines the `Error` enum used as the reconciler's error type.
- `src/controller.rs` — the whole reconciler:
  - `Instance` / `InstanceSpec` / `InstanceStatus` (the CRD, via `#[derive(CustomResource)]`).
  - `reconcile` wraps the user logic in `kube::runtime::finalizer` with `N8N_FINALIZER`.
    The finalizer guarantees `Instance::cleanup` runs on delete.
  - `Instance::reconcile` applies the child `Deployment` + `Service` (`build_deployment`,
    `build_service`) via SSA with field manager `n8n-rustful-operator`, emits an `Applied`
    event, then patches `.status` via `patch_status` (also SSA).
  - `State` is shared between web server and controller; `to_context` produces the per-reconcile
    `Context` containing `Client`, event `Recorder`, `Diagnostics`, `Metrics`.
- `src/metrics.rs` — Prometheus registry (`n8n_operator_reconcile` prefix). `ReconcileMeasurer`
  uses `Drop` to record duration; failure label `error` is `Error::metric_label()` (lowercased
  Debug). Failure label `instance` is `name_any()` of the offending CR — `"illegal"` triggers
  `Error::IllegalInstance` and is used in tests to assert the failure path.
- `src/telemetry.rs` — `tracing` subscriber with an `EnvFilter` reload handle (lets
  `PUT /log-level` rewrite the filter at runtime). OTLP/gRPC tracer is gated behind
  the `telemetry` feature.
- `src/crdgen.rs` — second binary; prints the CRD YAML produced by `Instance::crd()`.

### CRD naming

Group `n8n.slys.dev`, kind `Instance`, plural `instances` (overridden in the `#[kube(...)]`
attribute — without it, kube-derive would pluralize to `instanceofs`), shortname `n8n`.
So `kubectl get instance.n8n.slys.dev` reads "n8n instance"; `kubectl get n8n` also works.
The finalizer string `instances.n8n.slys.dev` mirrors the CRD name.

### Adding fields to the CRD

1. Edit `InstanceSpec` / `InstanceStatus` in `src/controller.rs`.
2. `just generate` to refresh `yaml/crd.yaml`.
3. `just install-crd` to update the live cluster.
4. If you added a status field, also update the `Patch::Apply(json!({...}))` block in
   `Instance::reconcile` — server-side apply will drop fields not present in the patch.

### Telemetry feature

`#[cfg(feature = "telemetry")]` gates OTLP wiring in `telemetry.rs`. The `unused_imports`
allow at the top of that file keeps the non-telemetry build clean. The web server is
identical in both feature configurations.

## Conventions

- Commit messages follow [Conventional Commits](https://www.conventionalcommits.org/) /
  semver: `feat:`, `fix:`, `chore:`, `refactor:`, `docs:`, `test:`, `build:`, `ci:`.
  Breaking changes use `feat!:` / `fix!:` or a `BREAKING CHANGE:` footer. The prefix drives
  the version bump (`feat` → minor, `fix` → patch, `!`/`BREAKING CHANGE` → major).
- `rustfmt.toml` uses nightly-only options (`imports_granularity`, `overflow_delimited_expr`).
  Always format with `cargo +nightly fmt` (or `just fmt`); plain `cargo fmt` will reject them.
- All k8s writes from this operator use server-side apply with field manager
  `n8n-rustful-operator`. Keep that consistent — switching managers mid-flight will leave
  orphaned managed fields.
- Reconciler errors must round-trip through `Error::FinalizerError(Box<...>)`. The boxing
  exists to break a cycle in the type — don't try to flatten it.
