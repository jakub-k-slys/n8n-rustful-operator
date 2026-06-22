[private]
default:
  @just --list --unsorted

# install crd into the cluster
install-crd: generate
  kubectl apply -f yaml/crd.yaml

generate:
  cargo run --bin crdgen > yaml/crd.yaml

# run with opentelemetry
run-telemetry:
  OPENTELEMETRY_ENDPOINT_URL=http://127.0.0.1:4317 RUST_LOG=info,kube=debug,n8n_rustful_operator=debug cargo run --features=telemetry

# run without opentelemetry
run:
  RUST_LOG=info,kube=debug,n8n_rustful_operator=debug cargo run

# format with stable rustfmt
fmt:
  cargo fmt

# run unit tests (the cucumber BDD suite needs a kind cluster — see e2e.yml)
test-unit:
  cargo test --lib --bins

# compile for musl (for docker image) — local build stages the amd64 binary;
# the multiarch (amd64 + arm64) image is built in CI by .github/workflows/docker.yml
compile features="":
  #!/usr/bin/env bash
  docker run --rm \
    -v cargo-cache:/root/.cargo \
    -v $PWD:/volume \
    -w /volume \
    -t clux/muslrust:stable \
    cargo build --release --features={{features}} --bin n8n-rustful-operator
  mkdir -p dist/linux/amd64
  cp target/x86_64-unknown-linux-musl/release/n8n-rustful-operator dist/linux/amd64/

[private]
_build features="":
  just compile {{features}}
  docker build --platform linux/amd64 -t jslys/n8n-rustful-operator:local .

build-base: (_build "")
build-otel: (_build "telemetry")
