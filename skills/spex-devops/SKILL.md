---
name: spex-devops
description: >
  Infrastructure and DevOps agent for the spex framework.
  Invoke when you need to set up Docker Compose, write a CI pipeline,
  configure the deployment, add observability, set up secrets management,
  write a runbook, create the staging environment, configure health checks,
  set up the CI/CD pipeline, or when you ask "what infrastructure do we need
  for this feature" or "help me containerize this".
  Use for any slice that requires new infrastructure components, CI/CD pipeline
  changes, environment configuration, container topology design, reverse proxy
  setup, log aggregation, distributed tracing, or operational runbook authoring.
---

# Skill: spex-devops

> **Core principle:** Every environment is reproducible from a single command, every secret is injected at runtime, every service has a health check.

## References

| File | Contents |
|------|----------|
| [references/mcp-protocol.md](references/mcp-protocol.md) | MCP State Check procedure, State Protocol snippets, runbook MCP storage pattern |
| [references/infra-patterns.md](references/infra-patterns.md) | Health checks, service labeling, port rules, reverse proxy, Dockerfile best practices, full Compose topology template |
| [references/observability.md](references/observability.md) | Prometheus + Grafana, Loki, OpenTelemetry Collector, alerting rules, Grafana dashboards, CI observability gates |
| [references/ci-cd.md](references/ci-cd.md) | GitHub Actions + GitLab CI canonical pipelines, cache strategies, matrix builds, deployment gates |
| [references/kubernetes.md](references/kubernetes.md) | Deployments, Services, Ingress, ConfigMaps, Secrets, HPA, resource limits, rolling update strategy |

---

## Deployment Target Decision Table

Choose the deployment model based on team size, traffic, and operational maturity:

| Scenario | Recommended model | Notes |
|---|---|---|
| Solo / small team, single server | **Docker Compose** on a VPS | Simplest operational model; Watchtower or Portainer for updates |
| Multi-service, moderate scale | **Docker Compose** + Caddy/Traefik | Automatic TLS, zero-downtime deploys with `--no-deps` rolling restarts |
| Multiple environments, auto-scaling | **Kubernetes** (k3s, EKS, GKE) | Add when you need HPA, node pools, or multi-region |
| Serverless / event-driven | **Cloud Functions / Lambda + managed DB** | Only when you have no long-running processes |
| Bare-metal or legacy migration | **Ansible + Systemd units** | Fall back when containers are not viable |

---

## CI/CD Platform Decision Table

| Platform | When to use | Key advantage |
|---|---|---|
| **GitHub Actions** | GitHub-hosted code | Native secrets, reusable workflows, marketplace |
| **GitLab CI** | GitLab-hosted code | Built-in registry, environments, SAST/DAST |
| **Bitbucket Pipelines** | Atlassian ecosystem | Native Jira integration |
| **Drone CI / Woodpecker** | Self-hosted, Docker-native | Ultra-light, minimal config |
| **Jenkins** | Legacy / enterprise | Only when org mandate; prefer any of the above |

---

## Reverse Proxy Decision Table

| Proxy | When to use | TLS automation |
|---|---|---|
| **Caddy** | Single server, zero-config HTTPS | Built-in ACME; default choice for Compose stacks |
| **Traefik** | Docker Compose / Kubernetes with dynamic service discovery | Label-driven config; great for Compose |
| **nginx** | High-traffic, custom routing, existing nginx expertise | Manual TLS or cert-bot; most tunable |
| **Kong** | API gateway needs (rate limiting, JWT validation) | Plugin ecosystem |
| **Cloudflare Tunnel** | No public inbound ports, CDN + DDoS protection | Tunnels outbound only |

---

## Container Registry Decision Table

| Registry | When to use |
|---|---|
| **GitHub Container Registry (ghcr.io)** | GitHub Actions CI — free for public, cheap for private |
| **GitLab Registry** | GitLab CI — integrated, zero config |
| **Docker Hub** | Public images only (rate limits for private) |
| **AWS ECR / GCR / ACR** | Cloud-native deployments on respective clouds |
| **Self-hosted Harbor** | Air-gapped or strict compliance environments |

---

## Infra Rules (non-negotiable)

- **No secrets in repo** — `.env.example` with placeholder values only; real secrets injected at runtime via environment or a secrets manager
- **No public internal ports** — databases, caches, and internal services must not be exposed outside the container network
- **Health checks required** — every service needs a health check; never use `sleep`; use `condition: service_healthy`
- **Reproducible and idempotent** — full teardown + rebuild must produce an identical environment; migrations and seeds must be safe to re-run
- **Single-command startup** — `docker compose up` (or equivalent) brings up the full local environment from scratch
- **HTTPS on all public endpoints** — never disable TLS on any internet-facing service
- **Pin image tags** — never use `latest` in staging or production; use a specific digest or version tag
- **Label every container/service** — `project`, `env`, and `component` labels are mandatory

---

## Canonical: Symfony + MariaDB Compose Stack

This is the reference stack for PHP/Symfony projects (the preferred project stack):

```yaml
# docker-compose.yml
services:

  proxy:
    image: caddy:2-alpine
    ports:
      - "80:80"
      - "443:443"
    volumes:
      - ./docker/caddy/Caddyfile:/etc/caddy/Caddyfile:ro
      - caddy_data:/data
      - caddy_config:/config
    networks: [public, internal]
    labels:
      project: "${COMPOSE_PROJECT_NAME}"
      env: "${APP_ENV:-development}"
      component: proxy
    restart: unless-stopped

  app:
    build:
      context: .
      dockerfile: docker/php/Dockerfile
      target: ${APP_BUILD_TARGET:-development}
    environment:
      APP_ENV: ${APP_ENV:-dev}
      APP_SECRET: ${APP_SECRET}
      DATABASE_URL: mysql://${DB_USER}:${DB_PASSWORD}@db:3306/${DB_NAME}?serverVersion=mariadb-10.11.0&charset=utf8mb4
    volumes:
      - .:/var/www/html:cached
      - /var/www/html/vendor
    depends_on:
      db:
        condition: service_healthy
    networks: [internal]
    labels:
      project: "${COMPOSE_PROJECT_NAME}"
      env: "${APP_ENV:-development}"
      component: app
    healthcheck:
      test: ["CMD", "php-fpm", "-t"]
      interval: 10s
      timeout: 5s
      retries: 5
      start_period: 30s

  db:
    image: mariadb:10.11
    environment:
      MARIADB_ROOT_PASSWORD: ${DB_ROOT_PASSWORD}
      MARIADB_DATABASE: ${DB_NAME}
      MARIADB_USER: ${DB_USER}
      MARIADB_PASSWORD: ${DB_PASSWORD}
    volumes:
      - db_data:/var/lib/mysql
      - ./docker/mariadb/conf.d:/etc/mysql/conf.d:ro
    networks: [internal]
    labels:
      project: "${COMPOSE_PROJECT_NAME}"
      env: "${APP_ENV:-development}"
      component: db
    healthcheck:
      test: ["CMD", "healthcheck.sh", "--connect", "--innodb_initialized"]
      interval: 10s
      timeout: 5s
      retries: 10
      start_period: 30s

  redis:
    image: redis:7-alpine
    command: redis-server --save 60 1 --loglevel warning
    volumes:
      - redis_data:/data
    networks: [internal]
    labels:
      project: "${COMPOSE_PROJECT_NAME}"
      env: "${APP_ENV:-development}"
      component: cache
    healthcheck:
      test: ["CMD", "redis-cli", "ping"]
      interval: 5s
      timeout: 3s
      retries: 5

networks:
  public:
    driver: bridge
  internal:
    driver: bridge

volumes:
  db_data:
  redis_data:
  caddy_data:
  caddy_config:
```

```
# docker/caddy/Caddyfile (local development with automatic TLS)
{$APP_DOMAIN:localhost} {
    reverse_proxy app:9000 {
        transport fastcgi {
            root /var/www/html/public
            env SERVER_NAME {$APP_DOMAIN:localhost}
        }
    }
    file_server
    root * /var/www/html/public
    encode gzip
    log {
        output stdout
        format json
    }
}
```

---

## Canonical: PHP/Symfony Dockerfile (multi-stage)

```dockerfile
# docker/php/Dockerfile
FROM php:8.3-fpm-alpine AS base

# System deps
RUN apk add --no-cache \
    icu-dev \
    libzip-dev \
    oniguruma-dev \
    && docker-php-ext-install -j$(nproc) \
        intl \
        pdo_mysql \
        zip \
        opcache \
        mbstring

# Install Composer
COPY --from=composer:2 /usr/bin/composer /usr/bin/composer

WORKDIR /var/www/html

# ---- Development stage ----
FROM base AS development

RUN apk add --no-cache $PHPIZE_DEPS \
    && pecl install xdebug \
    && docker-php-ext-enable xdebug

COPY docker/php/conf.d/xdebug.ini /usr/local/etc/php/conf.d/xdebug.ini

# ---- Production stage ----
FROM base AS production

ENV APP_ENV=prod
ENV COMPOSER_ALLOW_SUPERUSER=1

COPY composer.json composer.lock symfony.lock ./
RUN composer install --no-dev --optimize-autoloader --no-scripts --no-progress

COPY . .
RUN composer run-script post-install-cmd \
    && php bin/console cache:warmup \
    && chown -R www-data:www-data var/

COPY docker/php/conf.d/opcache.ini /usr/local/etc/php/conf.d/opcache.ini

USER www-data
EXPOSE 9000
```

---

## Inputs

| Input | Source | Required |
|-------|--------|----------|
| Architecture overview | Project vision artifact | yes |
| Slice infrastructure needs | Slice spec + backend/frontend specs | yes |
| Security requirements | `spex-qa` security review or human input | yes |
| Target runtime environment | Human or `docs/PRD.md` | yes |

---

## Process

1. **Read** the architecture overview and slice spec before designing anything; check MCP memory for prior decisions (see `references/mcp-protocol.md`)
2. **Identify** the target deployment model using the decision table above
3. **Design** the container/service topology; document service boundaries, network segments, and dependencies
4. **Write** configuration files — Compose, Dockerfiles, CI YAML, Kubernetes manifests, Terraform, or equivalent
5. **Configure** observability — metrics endpoint, distributed tracing, log aggregation (see `references/observability.md`)
6. **Write** CI/CD pipeline using the canonical patterns from `references/ci-cd.md`
7. **Write** operational runbooks covering deployment, rollback, and incident procedures; store in MCP (see `references/mcp-protocol.md`)
8. **Verify** the environment starts cleanly from a full teardown with a single command; confirm health checks pass
9. **Confirm** no secrets are in the repository and all infra rules are satisfied; record decisions in MCP memory

---

## Outputs

| Artifact | ID Pattern | Description |
|----------|-----------|-------------|
| `runbook` | `PROJ-OPS-NNN` | Operational procedure or deployment guide — stored in MCP only |

Infrastructure deliverables committed as source files:
- Container / Compose / Helm / Terraform configuration
- CI/CD pipeline definitions (`/.github/workflows/` or `/.gitlab-ci.yml`)
- Reverse proxy configuration (`docker/caddy/` or `docker/nginx/`)
- Observability collector and dashboard configuration (`docker/otel/`, `docker/prometheus/`)
- `.env.example` with placeholder values (no real secrets)
- `Makefile` with `make dev`, `make test`, `make build` targets

Runbooks are stored in MCP only — **do not commit to `docs/ops/`**:
```
artifact_register(id="PROJ-OPS-NNN", spec="SLICE-NNN", task="T0NN-N",
  agent="spex-devops", type="runbook", path="mcp:ops/PROJ-OPS-NNN")
memory_set(agent="spex-devops", key="artifact_PROJ-OPS-NNN", value=<runbook content>)
```

---

## Handoff

Report to `spex-orchestrate`:

```
AGENT: spex-devops
ARTIFACT: PROJ-OPS-NNN  type=runbook  status=review
GATE: make check [PASS|FAIL]
SUMMARY: <1-2 sentences on infra changes and environment verification result>
OPEN QUESTIONS: <list or "none">
```

---

## Git Protocol

Commit directly to the current branch (default dev flow):

```
git add <changed files>
git commit -m "feat(infra): <description> — Refs: TASK-NNN"
```

Never run `git push` — remote push is the human's decision.

---

## Delivery Checklist

- [ ] MCP state check completed at startup; prior context restored if available
- [ ] Slice spec and architecture overview read before writing any config
- [ ] Deployment model chosen and documented using the decision table
- [ ] Container/service topology documented with service boundaries and network segments
- [ ] All configuration files written (Compose, Dockerfiles, CI YAML, etc.)
- [ ] Health checks present on every service; no `sleep` used; condition-based waits only
- [ ] All containers/services labeled (`project`, `env`, `component`)
- [ ] No internal service ports exposed publicly
- [ ] Image tags pinned (no `latest` in staging/production)
- [ ] Observability configured: metrics endpoint, tracing, log aggregation
- [ ] CI/CD pipeline written with lint, build, security scan, smoke test, and teardown gates
- [ ] Runbook written and stored in MCP; `artifact_register` + `memory_set` called
- [ ] `.env.example` contains only placeholders; no real secrets anywhere in repo
- [ ] Single-command startup verified from clean teardown
- [ ] Deployments are reproducible and idempotent (teardown + rebuild tested)
- [ ] HTTPS enabled on all public-facing endpoints
- [ ] `session_context` written to MCP memory on task completion
- [ ] Handoff envelope reported to `spex-orchestrate`
- [ ] Commit created with `feat(infra):` prefix and task reference
