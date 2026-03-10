# Observability — spex-devops

## Observability Stack Decision Table

| Stack | When to use | Compose complexity |
|---|---|---|
| **Prometheus + Grafana** | Any project with custom metrics; on-prem or self-hosted | Medium — add 2–3 services |
| **Prometheus + Grafana + Loki + Tempo** | Full PLG (logs + metrics + traces in one UI) | Medium-high — the reference stack below |
| **OpenTelemetry Collector + Jaeger** | Traces-first; language-native instrumentation | Low — 2 services |
| **Datadog / New Relic** | Managed; enterprise; budget available | Zero — agent sidecar only |
| **AWS CloudWatch / GCP Cloud Monitoring** | Already on that cloud | Zero local config |

**Default recommendation:** Prometheus + Grafana + Loki + OpenTelemetry Collector for self-hosted projects.

---

## Full PLG Stack Compose Snippet

```yaml
# Add to docker-compose.yml (observability profile)
services:

  prometheus:
    image: prom/prometheus:v2.51.0
    command:
      - "--config.file=/etc/prometheus/prometheus.yml"
      - "--storage.tsdb.path=/prometheus"
      - "--storage.tsdb.retention.time=15d"
      - "--web.enable-lifecycle"
    volumes:
      - ./docker/prometheus/prometheus.yml:/etc/prometheus/prometheus.yml:ro
      - ./docker/prometheus/rules:/etc/prometheus/rules:ro
      - prometheus_data:/prometheus
    networks: [internal]
    labels:
      project: "${COMPOSE_PROJECT_NAME}"
      env: "${APP_ENV:-development}"
      component: metrics

  grafana:
    image: grafana/grafana:10.4.0
    environment:
      GF_SECURITY_ADMIN_USER: ${GRAFANA_ADMIN_USER:-admin}
      GF_SECURITY_ADMIN_PASSWORD: ${GRAFANA_ADMIN_PASSWORD}
      GF_SERVER_ROOT_URL: "https://${APP_DOMAIN}/grafana/"
      GF_SERVER_SERVE_FROM_SUB_PATH: "true"
    volumes:
      - grafana_data:/var/lib/grafana
      - ./docker/grafana/provisioning:/etc/grafana/provisioning:ro
      - ./docker/grafana/dashboards:/var/lib/grafana/dashboards:ro
    depends_on:
      - prometheus
      - loki
    networks: [internal]
    labels:
      project: "${COMPOSE_PROJECT_NAME}"
      env: "${APP_ENV:-development}"
      component: dashboard

  loki:
    image: grafana/loki:2.9.5
    command: -config.file=/etc/loki/config.yml
    volumes:
      - ./docker/loki/config.yml:/etc/loki/config.yml:ro
      - loki_data:/loki
    networks: [internal]
    labels:
      project: "${COMPOSE_PROJECT_NAME}"
      env: "${APP_ENV:-development}"
      component: logs

  promtail:
    image: grafana/promtail:2.9.5
    command: -config.file=/etc/promtail/config.yml
    volumes:
      - /var/lib/docker/containers:/var/lib/docker/containers:ro
      - /var/run/docker.sock:/var/run/docker.sock:ro
      - ./docker/promtail/config.yml:/etc/promtail/config.yml:ro
    depends_on:
      - loki
    networks: [internal]

  otel-collector:
    image: otel/opentelemetry-collector-contrib:0.99.0
    command: ["--config=/etc/otel/config.yml"]
    volumes:
      - ./docker/otel/config.yml:/etc/otel/config.yml:ro
    networks: [internal]
    labels:
      project: "${COMPOSE_PROJECT_NAME}"
      env: "${APP_ENV:-development}"
      component: tracing

volumes:
  prometheus_data:
  grafana_data:
  loki_data:
```

---

## Prometheus Configuration

```yaml
# docker/prometheus/prometheus.yml
global:
  scrape_interval: 15s
  evaluation_interval: 15s
  external_labels:
    environment: "${APP_ENV}"
    project: "${COMPOSE_PROJECT_NAME}"

rule_files:
  - "rules/*.yml"

scrape_configs:
  - job_name: "app"
    static_configs:
      - targets: ["app:9100"]
    metrics_path: /metrics
    scrape_interval: 15s

  - job_name: "worker"
    static_configs:
      - targets: ["worker:9100"]

  - job_name: "mariadb"
    static_configs:
      - targets: ["mariadb-exporter:9104"]

  - job_name: "redis"
    static_configs:
      - targets: ["redis-exporter:9121"]

  - job_name: "node"
    static_configs:
      - targets: ["node-exporter:9100"]
    relabel_configs:
      - source_labels: [__address__]
        target_label: instance

  - job_name: "caddy"
    static_configs:
      - targets: ["proxy:2019"]
    metrics_path: /metrics
```

### Alerting Rules

```yaml
# docker/prometheus/rules/api.yml
groups:
  - name: api
    rules:
      - alert: HighErrorRate
        expr: |
          sum(rate(http_requests_total{status=~"5.."}[5m]))
          / sum(rate(http_requests_total[5m])) > 0.01
        for: 5m
        labels:
          severity: warning
        annotations:
          summary: "High HTTP 5xx error rate — {{ $value | humanizePercentage }}"
          runbook: "mcp:ops/PROJ-OPS-001"

      - alert: HighErrorRateCritical
        expr: |
          sum(rate(http_requests_total{status=~"5.."}[2m]))
          / sum(rate(http_requests_total[2m])) > 0.05
        for: 2m
        labels:
          severity: critical
        annotations:
          summary: "Critical HTTP 5xx error rate — {{ $value | humanizePercentage }}"

      - alert: SlowP99Latency
        expr: |
          histogram_quantile(0.99,
            sum(rate(http_request_duration_seconds_bucket[5m])) by (le, service)
          ) > 1
        for: 5m
        labels:
          severity: warning
        annotations:
          summary: "P99 latency above 1s on {{ $labels.service }}"

      - alert: ServiceDown
        expr: up == 0
        for: 1m
        labels:
          severity: critical
        annotations:
          summary: "Service {{ $labels.job }} is down"

  - name: infrastructure
    rules:
      - alert: HighCpuUsage
        expr: 100 - (avg by(instance) (irate(node_cpu_seconds_total{mode="idle"}[5m])) * 100) > 90
        for: 5m
        labels:
          severity: critical

      - alert: HighMemoryUsage
        expr: (1 - (node_memory_MemAvailable_bytes / node_memory_MemTotal_bytes)) * 100 > 95
        for: 5m
        labels:
          severity: critical

      - alert: DiskAlmostFull
        expr: (node_filesystem_avail_bytes{fstype!="tmpfs"} / node_filesystem_size_bytes) * 100 < 10
        for: 10m
        labels:
          severity: warning

      - alert: MariaDBDown
        expr: mysql_up == 0
        for: 1m
        labels:
          severity: critical
```

### Alerting Threshold Reference

| Signal | Warning | Critical |
|---|---|---|
| HTTP 5xx error rate | > 1% over 5 min | > 5% over 2 min |
| P99 request latency | > 1 s | > 3 s |
| CPU utilization | > 70% sustained 10 min | > 90% sustained 5 min |
| Memory utilization | > 80% | > 95% |
| Disk usage | > 75% | > 90% |
| Queue depth | > 1 000 messages | > 10 000 messages |
| DB connection pool | > 70% | > 90% |

---

## Loki Configuration

```yaml
# docker/loki/config.yml
auth_enabled: false

server:
  http_listen_port: 3100
  grpc_listen_port: 9096

common:
  instance_addr: 127.0.0.1
  path_prefix: /loki
  storage:
    filesystem:
      chunks_directory: /loki/chunks
      rules_directory: /loki/rules
  replication_factor: 1
  ring:
    kvstore:
      store: inmemory

query_range:
  results_cache:
    cache:
      embedded_cache:
        enabled: true
        max_size_mb: 100

schema_config:
  configs:
    - from: 2024-01-01
      store: tsdb
      object_store: filesystem
      schema: v13
      index:
        prefix: index_
        period: 24h

ruler:
  alertmanager_url: http://alertmanager:9093

limits_config:
  ingestion_rate_mb: 4
  ingestion_burst_size_mb: 6
  retention_period: 744h   # 31 days
```

```yaml
# docker/promtail/config.yml
server:
  http_listen_port: 9080
  grpc_listen_port: 0

positions:
  filename: /tmp/positions.yaml

clients:
  - url: http://loki:3100/loki/api/v1/push

scrape_configs:
  - job_name: docker
    docker_sd_configs:
      - host: unix:///var/run/docker.sock
        refresh_interval: 5s
        filters:
          - name: label
            values: ["project=${COMPOSE_PROJECT_NAME}"]
    relabel_configs:
      - source_labels: [__meta_docker_container_name]
        target_label: container
      - source_labels: [__meta_docker_container_label_component]
        target_label: component
      - source_labels: [__meta_docker_container_label_env]
        target_label: env
    pipeline_stages:
      - json:
          expressions:
            level: level
            trace_id: trace_id
      - labels:
          level:
          trace_id:
```

---

## OpenTelemetry Collector Configuration

```yaml
# docker/otel/config.yml
receivers:
  otlp:
    protocols:
      grpc:
        endpoint: 0.0.0.0:4317
      http:
        endpoint: 0.0.0.0:4318

processors:
  batch:
    timeout: 1s
    send_batch_size: 1024
  memory_limiter:
    check_interval: 1s
    limit_mib: 512
    spike_limit_mib: 128
  resource:
    attributes:
      - key: deployment.environment
        value: "${APP_ENV}"
        action: upsert

exporters:
  # Send traces to Tempo (or Jaeger)
  otlp/tempo:
    endpoint: tempo:4317
    tls:
      insecure: true

  # Send metrics to Prometheus (via remote_write or as scrape target)
  prometheus:
    endpoint: "0.0.0.0:8889"

  # Debug exporter for development
  debug:
    verbosity: detailed

service:
  pipelines:
    traces:
      receivers: [otlp]
      processors: [memory_limiter, batch]
      exporters: [otlp/tempo]
    metrics:
      receivers: [otlp]
      processors: [memory_limiter, batch]
      exporters: [prometheus]
```

### App instrumentation env vars (add to every service)

```yaml
environment:
  OTEL_SERVICE_NAME: "app"
  OTEL_EXPORTER_OTLP_ENDPOINT: "http://otel-collector:4317"
  OTEL_EXPORTER_OTLP_PROTOCOL: "grpc"
  OTEL_TRACES_SAMPLER: "parentbased_traceidratio"
  OTEL_TRACES_SAMPLER_ARG: "0.1"      # 10% in production; 1.0 in development
  OTEL_PROPAGATORS: "tracecontext,baggage"
  OTEL_RESOURCE_ATTRIBUTES: "deployment.environment=${APP_ENV},service.version=${APP_VERSION}"
```

---

## Grafana Provisioning

```yaml
# docker/grafana/provisioning/datasources/datasources.yml
apiVersion: 1

datasources:
  - name: Prometheus
    type: prometheus
    access: proxy
    url: http://prometheus:9090
    isDefault: true
    jsonData:
      timeInterval: "15s"

  - name: Loki
    type: loki
    access: proxy
    url: http://loki:3100
    jsonData:
      maxLines: 1000

  - name: Tempo
    type: tempo
    access: proxy
    url: http://tempo:3200
    jsonData:
      tracesToLogsV2:
        datasourceUid: loki
        filterByTraceID: true
      serviceMap:
        datasourceUid: prometheus
      nodeGraph:
        enabled: true
```

```yaml
# docker/grafana/provisioning/dashboards/dashboards.yml
apiVersion: 1

providers:
  - name: default
    type: file
    disableDeletion: false
    updateIntervalSeconds: 30
    options:
      path: /var/lib/grafana/dashboards
```

---

## Structured Log Format

- Structured JSON logs only — no unstructured free-text in production.
- Required fields per log entry: `timestamp` (ISO-8601), `level`, `service`, `trace_id`, `span_id`, `message`.
- Do not log secrets, PII, or full request/response bodies.

```json
{
  "timestamp": "2026-03-10T12:34:56.789Z",
  "level": "error",
  "service": "api",
  "trace_id": "4bf92f3577b34da6a3ce929d0e0e4736",
  "span_id": "00f067aa0ba902b7",
  "message": "Failed to process order",
  "order_id": "ord_123",
  "error": "connection refused"
}
```

### Symfony Monolog JSON formatter

```yaml
# config/packages/prod/monolog.yaml
monolog:
  handlers:
    main:
      type: stream
      path: php://stdout
      level: info
      formatter: monolog.formatter.json
      channels: ["!event"]
```

---

## Metrics Endpoint Requirements

- Every service **must** expose a `/metrics` endpoint in Prometheus exposition format.
- Endpoint must be accessible on the internal network only (not through the public proxy).
- Use a dedicated secondary port (e.g. `9100`) or the same app port with path separation.

Key metrics every service should expose (RED method):
- **Rate** — requests per second: `http_requests_total{method, path, status}`
- **Error** — error rate: `http_requests_total{status=~"5.."}`
- **Duration** — response time histogram: `http_request_duration_seconds_bucket{le}`

Also expose:
- Saturation: queue depth, DB connection pool usage, worker concurrency
- Business metrics: e.g. `orders_processed_total`, `payment_failures_total`

---

## CI/CD Observability Gates

Include the following gates in every CI pipeline (see `references/ci-cd.md` for full pipeline):

1. **Lint** — lint Dockerfiles (`hadolint`), IaC (`tflint`, `kube-linter`), and CI YAML
2. **Build** — build all container images; fail fast on build errors
3. **Security scan** — scan images with `trivy`; fail on CRITICAL vulnerabilities
4. **Smoke test** — start the Compose stack, wait for all health checks, run a minimal request suite
5. **Observability check** — assert `/metrics` returns HTTP 200 and contains expected metric names
6. **Teardown** — always run `docker compose down -v` in a `finally`/`post` step

```yaml
# GitHub Actions smoke + observability check
- name: Start stack and wait for health
  run: docker compose up -d --wait --timeout 120

- name: Smoke test
  run: |
    curl -sf http://localhost/healthz
    curl -sf http://localhost/api/ping

- name: Observability check
  run: |
    curl -sf http://localhost:9100/metrics | grep -q 'http_requests_total'

- name: Teardown
  if: always()
  run: docker compose down -v
```
