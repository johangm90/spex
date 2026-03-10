# Infra Patterns — spex-devops

## Health Check Requirements

- Every service **must** declare a health check — no exceptions.
- **Never use `sleep`** as a readiness mechanism.
- Use `condition: service_healthy` in `depends_on` to sequence startup.

```yaml
# MariaDB health check (preferred for Symfony projects)
db:
  image: mariadb:10.11
  healthcheck:
    test: ["CMD", "healthcheck.sh", "--connect", "--innodb_initialized"]
    interval: 10s
    timeout: 5s
    retries: 10
    start_period: 30s

# PostgreSQL health check
db:
  image: postgres:16-alpine
  healthcheck:
    test: ["CMD-SHELL", "pg_isready -U $$POSTGRES_USER -d $$POSTGRES_DB"]
    interval: 5s
    timeout: 5s
    retries: 10
    start_period: 10s

# Redis health check
redis:
  image: redis:7-alpine
  healthcheck:
    test: ["CMD", "redis-cli", "ping"]
    interval: 5s
    timeout: 3s
    retries: 5

# PHP-FPM health check
app:
  healthcheck:
    test: ["CMD", "php-fpm", "-t"]
    interval: 10s
    timeout: 5s
    retries: 5
    start_period: 30s

# Node.js HTTP health check
api:
  healthcheck:
    test: ["CMD-SHELL", "wget -qO- http://localhost:3000/healthz || exit 1"]
    interval: 10s
    timeout: 5s
    retries: 5
    start_period: 20s

# Upstream dependency check pattern
app:
  depends_on:
    db:
      condition: service_healthy
    redis:
      condition: service_healthy
    migrations:
      condition: service_completed_successfully
```

---

## Dockerfile Best Practices

### Multi-stage build principles

1. **`base` stage** — OS deps + runtime, no app code
2. **`development` stage** — adds dev tools (Xdebug, hot reload, etc.)
3. **`test` stage** — copies code and runs tests in CI
4. **`production` stage** — optimized, minimal, non-root user

```dockerfile
# Generic multi-stage pattern
FROM node:20-alpine AS base
WORKDIR /app
RUN addgroup -S appgroup && adduser -S appuser -G appgroup

FROM base AS development
RUN npm install -g nodemon
COPY package*.json ./
RUN npm install
CMD ["nodemon", "src/index.ts"]

FROM base AS deps
COPY package*.json ./
RUN npm ci --omit=dev

FROM base AS production
ENV NODE_ENV=production
COPY --from=deps /app/node_modules ./node_modules
COPY --chown=appuser:appgroup . .
USER appuser
EXPOSE 3000
CMD ["node", "dist/index.js"]
```

### Layer caching rules

- Copy `package*.json` / `composer.json` BEFORE `COPY . .`
- Install dependencies BEFORE copying source — unchanged deps layer survives a code change
- `RUN` commands that change rarely (OS packages) go first; code-specific commands go last

```dockerfile
# CORRECT — deps cached separately from source
COPY package.json package-lock.json ./
RUN npm ci
COPY . .
RUN npm run build

# WRONG — any source change invalidates the npm install layer
COPY . .
RUN npm ci && npm run build
```

### Security rules

- Never run as `root` in production — always switch to a non-root user
- Remove build tools not needed at runtime (`RUN apk del build-base`)
- Use `COPY --chown=user:group` to set ownership in one layer
- Scan every image in CI with `trivy` or `grype` before push

---

## Service Labeling

Label every container with at minimum `project`, `env`, and `component`:

```yaml
labels:
  project: "${COMPOSE_PROJECT_NAME}"
  env: "${APP_ENV:-development}"    # development | staging | production
  component: "api"                   # api | worker | db | cache | proxy | queue
```

This enables filtering with `docker ps --filter label=project=myapp` and integration with management tools (Portainer, Rancher, etc.).

---

## Single-Command Startup

The entire local environment must start from a clean slate with one command:

```bash
docker compose up --build
# or via Makefile alias:
make dev
```

Requirements:
- All dependent services declared and ordered via `depends_on` + `condition: service_healthy`
- Migrations run automatically on first boot (via a `migrations` init container)
- No manual steps, no separate `docker pull`, no hand-editing of config files

### Init container pattern for migrations

```yaml
services:
  migrations:
    build:
      context: .
      target: production
    command: php bin/console doctrine:migrations:migrate --no-interaction
    environment:
      DATABASE_URL: mysql://${DB_USER}:${DB_PASSWORD}@db:3306/${DB_NAME}?serverVersion=mariadb-10.11.0&charset=utf8mb4
    depends_on:
      db:
        condition: service_healthy
    networks: [internal]
    restart: "no"   # run once then exit

  app:
    depends_on:
      db:
        condition: service_healthy
      migrations:
        condition: service_completed_successfully
```

---

## Reproducible and Idempotent Deployments

- A full `docker compose down -v && docker compose up --build` must produce an **identical** working environment.
- Database migrations must be **idempotent** — safe to re-run without side effects.
- Seed scripts must be guarded by existence checks (do not re-insert if data exists).
- Pin image tags — never use `latest` in production or staging configs; use a specific digest or version tag.
- Infrastructure-as-code (Terraform, Pulumi, etc.) must be idempotent by design.

```yaml
# CORRECT — pinned version tag
image: mariadb:10.11.6

# WRONG — will silently pull a breaking update
image: mariadb:latest
```

---

## Reverse Proxy Patterns

### Caddy (default choice for single-server Compose stacks)

```
# Caddyfile — automatic HTTPS via ACME
{$APP_DOMAIN:localhost} {
    reverse_proxy app:9000 {
        transport fastcgi {
            root /var/www/html/public
        }
    }
    file_server
    root * /var/www/html/public
    encode gzip zstd
    log {
        output stdout
        format json
    }
    header {
        Strict-Transport-Security "max-age=31536000; includeSubDomains; preload"
        X-Content-Type-Options nosniff
        X-Frame-Options DENY
        Referrer-Policy strict-origin-when-cross-origin
    }
}
```

```yaml
# Caddy Compose service
proxy:
  image: caddy:2-alpine
  ports:
    - "80:80"
    - "443:443"
    - "443:443/udp"   # HTTP/3
  volumes:
    - ./docker/caddy/Caddyfile:/etc/caddy/Caddyfile:ro
    - caddy_data:/data
    - caddy_config:/config
  networks: [public, internal]
```

### Traefik (Docker-native, label-driven)

```yaml
# traefik.yml
api:
  dashboard: true
  insecure: false   # never true in production

providers:
  docker:
    exposedByDefault: false
    network: internal

entryPoints:
  web:
    address: ":80"
    http:
      redirections:
        entrypoint:
          to: websecure
          scheme: https
  websecure:
    address: ":443"

certificatesResolvers:
  letsencrypt:
    acme:
      email: ${ACME_EMAIL}
      storage: /letsencrypt/acme.json
      httpChallenge:
        entryPoint: web
```

```yaml
# App service labels for Traefik
labels:
  traefik.enable: "true"
  traefik.http.routers.app.rule: "Host(`${APP_DOMAIN}`)"
  traefik.http.routers.app.entrypoints: "websecure"
  traefik.http.routers.app.tls.certresolver: "letsencrypt"
  traefik.http.services.app.loadbalancer.server.port: "9000"
```

### nginx (high-traffic, maximum tunability)

```nginx
upstream api {
    server api:8000;
    keepalive 32;
}

server {
    listen 443 ssl http2;
    server_name app.example.com;

    ssl_certificate     /etc/ssl/certs/app.crt;
    ssl_certificate_key /etc/ssl/private/app.key;
    ssl_protocols       TLSv1.2 TLSv1.3;
    ssl_ciphers         HIGH:!aNULL:!MD5;

    gzip on;
    gzip_types text/plain application/json application/javascript text/css;

    location /api/ {
        proxy_pass         http://api/;
        proxy_set_header   Host              $host;
        proxy_set_header   X-Real-IP         $remote_addr;
        proxy_set_header   X-Forwarded-For   $proxy_add_x_forwarded_for;
        proxy_set_header   X-Forwarded-Proto $scheme;
        proxy_read_timeout 30s;
        proxy_connect_timeout 5s;
        proxy_send_timeout 30s;
    }
}

server {
    listen 80;
    server_name app.example.com;
    return 301 https://$host$request_uri;
}
```

---

## Port Exposure Rules

| Service type | Expose to host? | Notes |
|---|---|---|
| Reverse proxy HTTP/HTTPS | **Yes** | Ports 80 and 443 only |
| API / app server | **No** | Reachable via proxy on internal network only |
| Database (MariaDB, PostgreSQL) | **No** | Internal network only; never expose 3306/5432 |
| Cache (Redis, Memcached) | **No** | Internal network only; never expose 6379/11211 |
| Message broker (RabbitMQ, Kafka) | **No** | Internal network only |
| Metrics collector (Prometheus) | **No** | Scrape internally; expose Grafana via proxy with auth |
| Admin UI (pgAdmin, phpMyAdmin) | **Dev only** | Never in staging or production |

```yaml
networks:
  internal:
    driver: bridge
  public:
    driver: bridge

services:
  proxy:
    networks: [internal, public]
    ports:
      - "80:80"
      - "443:443"
  app:
    networks: [internal]   # no ports: key
  db:
    networks: [internal]   # no ports: key
  redis:
    networks: [internal]   # no ports: key
```

---

## Secrets Management

### Local development — `.env` file pattern

```bash
# .env.example (committed to repo — placeholders only)
APP_ENV=dev
APP_SECRET=change_me_in_env_file
DB_NAME=myapp
DB_USER=myapp
DB_PASSWORD=change_me
DB_ROOT_PASSWORD=change_me_root
REDIS_URL=redis://redis:6379

# .env (never committed — real secrets, ignored via .gitignore)
APP_SECRET=a1b2c3d4e5f6...
DB_PASSWORD=super_secret_password
```

### Production — Docker Secrets (Swarm mode)

```yaml
services:
  db:
    environment:
      MARIADB_PASSWORD_FILE: /run/secrets/db_password
    secrets:
      - db_password

secrets:
  db_password:
    external: true
```

### Production — environment injection via CI/CD

- GitHub Actions: `secrets.DB_PASSWORD` → `env:` in workflow step
- GitLab CI: protected CI/CD variables
- Kubernetes: `Secret` objects mounted as env vars or volumes (see `references/kubernetes.md`)

---

## Makefile Targets (standard interface)

```makefile
.DEFAULT_GOAL := help

.PHONY: help dev build test lint down clean

help:           ## Show this help
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | sort | awk 'BEGIN {FS = ":.*?## "}; {printf "\033[36m%-20s\033[0m %s\n", $$1, $$2}'

dev:            ## Start development environment
	docker compose up --build

build:          ## Build production images
	docker compose -f docker-compose.prod.yml build

test:           ## Run test suite in Docker
	docker compose run --rm app php bin/phpunit

lint:           ## Lint Dockerfiles and CI config
	hadolint docker/php/Dockerfile
	docker compose config -q

down:           ## Stop and remove containers
	docker compose down

clean:          ## Full teardown including volumes
	docker compose down -v --remove-orphans
```
