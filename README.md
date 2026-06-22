# n8n-rustful-operator

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![CI](https://github.com/jakub-k-slys/n8n-rustful-operator/actions/workflows/ci.yml/badge.svg)](https://github.com/jakub-k-slys/n8n-rustful-operator/actions/workflows/ci.yml)
[![E2E](https://github.com/jakub-k-slys/n8n-rustful-operator/actions/workflows/e2e.yml/badge.svg)](https://github.com/jakub-k-slys/n8n-rustful-operator/actions/workflows/e2e.yml)

Kubernetes operator in Rust that runs [n8n](https://n8n.io) from custom resources.
Declare an n8n instance — single process or queue-mode cluster — and the operator
reconciles the Deployments, Services, Secrets and networking for you. Built on
[`kube-rs`](https://github.com/kube-rs/kube).

## Features

- **Two custom resources** — `Single` for one standalone n8n process, `Cluster` for
  [queue mode](https://docs.n8n.io/hosting/scaling/queue-mode/) with separate main,
  worker and webhook roles.
- **Databases** — SQLite (default), PostgreSQL, MySQL/MariaDB, with passwords and TLS
  pulled from Secrets.
- **Networking** — expose instances through an Ingress or a Gateway API HTTPRoute.
- **Encryption keys** — reference your own Secret or let the operator generate one.
- **Persistence** — provision a PVC at `/home/node/.n8n` for binary data and SQLite.
- **Autoscaling** — opt workers into an HorizontalPodAutoscaler.
- Server-side apply, owner references for garbage collection, recommended
  `app.kubernetes.io/*` labels, Prometheus metrics and OpenTelemetry tracing.

## Installation

Install the CRDs and the operator (Namespace, RBAC and Deployment):

```bash
kubectl apply -f yaml/crd.yaml
kubectl apply -f yaml/install.yaml   # replace __IMAGE_TAG__ with a released tag
```

Then apply an instance:

```bash
kubectl apply -f yaml/single-sample.yaml    # a single n8n process
kubectl apply -f yaml/cluster-sample.yaml   # queue-mode cluster
```

`kubectl get n8n` lists `Single`s; `kubectl get n8nc` lists `Cluster`s.

## Example

```yaml
apiVersion: n8n.slys.dev/v1
kind: Single
metadata:
  name: demo
spec:
  image: n8nio/n8n:latest
  replicas: 1
  host: n8n.example.com
  networking:
    ingress:
      className: nginx
      tlsSecretName: n8n-tls
```

## Development

A `justfile` wraps the common flows (`cargo`/`kubectl` work too):

```bash
just generate     # write yaml/crd.yaml from the Rust types
just install-crd  # generate + apply the CRDs
just run          # run the operator against the current kube context
just test-unit    # cargo test
just fmt          # cargo fmt
```

The operator exposes HTTP on `:8080`: `/` (diagnostics), `/health`, `/metrics`
(Prometheus), and `PUT /log-level`. See [CLAUDE.md](CLAUDE.md) for the source layout
and conventions.

## Author

Built by [Jakub Slys](https://iam.slys.dev) — Backend Engineer building distributed
systems for telecoms, running a self-hosted Kubernetes homelab, and building AI
automation pipelines with n8n, MCP, and Claude.

I write about building this kind of tooling — n8n self-hosting, Kubernetes operators,
and the engineering decisions behind them — at [iam.slys.dev](https://iam.slys.dev).

→ [iam.slys.dev](https://iam.slys.dev)

## License

MIT
