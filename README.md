# n8n-rustful-operator

Kubernetes operator (in Rust, on `kube-rs`) that reconciles `Instance` custom resources
into running [n8n](https://n8n.io) deployments. Modelled on
[`kube-rs/controller-rs`](https://github.com/kube-rs/controller-rs).

## CRD

`Instance` (`n8n.slys.dev/v1`, namespaced, shortname `n8n`) — describes a single n8n
deployment. Spec fields: `image`, `replicas`, optional `host`. The reconciler creates
a `Deployment` and `Service` per instance and reports back through `.status`.

## Run

```sh
just generate         # write yaml/crd.yaml from the Rust type
just install-crd      # apply CRD to the current kube context
just run              # run the operator against the current kube context
kubectl apply -f yaml/instance-sample.yaml
```

## Endpoints

The operator exposes HTTP on `:8080`:

- `GET /` — diagnostics
- `GET /health` — liveness
- `GET /metrics` — Prometheus
- `PUT /log-level` — runtime log filter (`{"filter": "info,kube=debug"}`)

## Telemetry

Build with `--features=telemetry` and set `OPENTELEMETRY_ENDPOINT_URL` to ship traces via OTLP/gRPC.
