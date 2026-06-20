FROM cgr.dev/chainguard/static
# TARGETARCH is provided by buildx (amd64 / arm64). The matching statically
# linked binary is cross-compiled and staged under dist/linux/<arch>/ before build.
ARG TARGETARCH
COPY --chown=nonroot:nonroot ./dist/linux/${TARGETARCH}/n8n-rustful-operator /app/
EXPOSE 8080
ENTRYPOINT ["/app/n8n-rustful-operator"]
