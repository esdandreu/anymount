# Provider telemetry via OpenTelemetry (OTLP)

**Date:** 2026-03-20

**Status:** Draft

Observability is **export-based** using standard OTLP so
any compatible collector or vendor backend can persist and query **logs,
traces, and metrics**.

## Summary

Named provider processes (`anymount provide --name <NAME>`) optionally enable an
**OpenTelemetry pipeline** that sends telemetry to a user-configured **OTLP
endpoint** (gRPC and/or HTTP, per OTLP spec). Configuration lives in **provider
config** (and may be **overridden or augmented** by standard `OTEL_*`
environment variables for compatibility with existing tooling).

The implementation builds on **`tracing`** (already used across the codebase)
and official **`opentelemetry-rust`** + **`opentelemetry-otlp`** exporters so that:

- **Traces** map from `tracing` spans (and `Logger::in_span`) to OTel traces.
- **Logs** map from `tracing` events / log records to the OTel Logs data model
  and export via OTLP (exact crate/layer choices follow current
  `opentelemetry-rust` recommendations at implementation time).
- **Metrics** use the OTel Metrics SDK with explicit instruments (counters,
  histograms); they do **not** appear automatically from log lines—add
  instruments where product value is clear (see phased rollout below).

Downstream, operators run **OpenTelemetry Collector**, **Grafana Alloy**,
**Jaeger**, **Tempo**, **Prometheus** (via collector), **Loki** (via collector),
vendor SaaS, etc.—**no anymount-specific log server**.

## Goals

- **Standard wire format:** OTLP only for export (no proprietary log protocol).
- **Portable operations:** one collector endpoint per environment; many
  providers can share it with distinct resource attributes.
- **Compatible with existing OTel tooling:** honor common `OTEL_*` env vars
  where practical (endpoint, headers, service name, resource attributes).
- **Per-provider identity** on the wire: resource attributes distinguish
  `provider.name` (and similar) so backends can filter.

## Non-goals (initial delivery)

- Replacing local **stdout / file** logging entirely (keep for dev and
  troubleshooting; OTel is additive unless config says otherwise).
- **Live tail inside anymount** without an external tool (use collector +
  Grafana/Jaeger/etc., or `otel-cli`/collector debug exporters).
- Automatic **metrics** for every internal function (too noisy); start with a
  small, intentional set.

## Configuration

### Provider TOML (primary product surface)

Extend `<name>.toml` with an optional section, for example:

```toml
[telemetry.otlp]
# Omit entire section or set enabled = false to disable OTLP export.
enabled = true
endpoint = "http://localhost:4317"   # OTLP gRPC default port; or https URL for HTTP/protobuf
protocol = "grpc"                    # "grpc" | "http/protobuf" (exact enum TBD in impl)

# Optional: auth and extra headers (sensitive; document file permissions)
# [telemetry.otlp.headers]
# Authorization = "Bearer ..."

# Optional: extra resource attributes (merged with defaults below)
# [telemetry.otlp.resource_attributes]
# deployment.environment = "staging"
```

**Defaults when `enabled = true` and fields omitted:** follow
`opentelemetry-otlp` / SDK defaults, merged with **standard environment
variables** (`OTEL_EXPORTER_OTLP_ENDPOINT`, signal-specific endpoints, `OTEL_*`
headers, etc.) so deployments can configure without editing TOML.

### Resource attributes (required semantics)

Every named provider export should include at minimum:

| Attribute | Example | Purpose |
|-----------|---------|---------|
| `service.name` | `anymount-provider` | Service identity in backends |
| `anymount.provider.name` | config name | Filter per provider |

Optional: `service.version` from crate version, `service.namespace` = `anymount`.

Use **OpenTelemetry semantic conventions** where they fit; custom keys under
`anymount.*` for product-specific dimensions.

### Process model

Each `provide --name` is a **separate OS process**, so OTel SDK initialization
runs **once per provider process** with that provider’s merged config + env.
No cross-process sharing of exporters is required.

## Implementation phases

### Phase 1 — OTLP traces + logs (MVP)

- Add OpenTelemetry Rust dependencies (SDK + OTLP exporter + tracing
  integration).
- On named-provider startup, if telemetry enabled: build `TracerProvider` and
  `LoggerProvider` (or equivalent in the SDK version in use), register global
  providers, install `tracing_subscriber` layers **alongside** existing fmt/file
  layers.
- Wire `TracingLogger` / existing `tracing::` calls into OTel without changing
  every call site first (bridge layer).
- Document example: run **OpenTelemetry Collector** locally, Grafana stack, or
  vendor receiver URL.

### Phase 2 — Metrics

- Introduce a small set of instruments (e.g. provider uptime, operation counters
  for mount/I/O paths that are already strategic). Export via OTLP metrics
  exporter to the same or a dedicated metrics endpoint per env vars.

### Phase 3 — Quality and ops

- Sampling hooks (`OTEL_TRACES_SAMPLER`, parent-based).
- Shutdown: flush exporters on graceful `disconnect` / process exit (`shutdown`
  with timeout).
- Document security: TLS, header secrets, never log exporter headers.

## Testing

- **Unit:** config parse / merge with env (where testable without network).
- **Integration:** optional `tests` behind feature flag or CI job that starts a
  mock OTLP receiver (e.g. collector in Docker or a minimal test server) and
  asserts export of at least one span and one log record for a short-lived
  `provide --name` smoke path.

## Risks / notes

- **Async runtime:** OTLP export is typically async; align with existing
  `tokio` usage on Linux and ensure non-Linux targets either enable a minimal
  runtime for export or use blocking exporters if appropriate for the crate
  version.
- **Dependency weight:** OpenTelemetry crates are heavy; gate with a Cargo
  **feature** (e.g. `otlp`) if binary size or compile time matters for some
  builds.
- **Crate API churn:** `opentelemetry-rust` evolves; pin versions and follow
  upstream migration guides when bumping.

## Local vs exported telemetry

Operators use **standard OTLP backends** for persistence and inspection;
developers keep **console/file** `tracing` layers for local troubleshooting.
