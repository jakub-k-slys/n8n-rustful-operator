FROM cgr.dev/chainguard/static
COPY --chown=nonroot:nonroot ./n8n-rustful-operator /app/
EXPOSE 8080
ENTRYPOINT ["/app/n8n-rustful-operator"]
