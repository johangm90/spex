# CI/CD — spex-devops

## Pipeline Stage Reference

Every pipeline — regardless of platform — must include these stages in order:

| Stage | Purpose | Fail behavior |
|---|---|---|
| **lint** | Static checks: Dockerfile, YAML, PHP/JS/TS | Fail immediately |
| **test** | Unit + integration tests | Fail immediately |
| **build** | Build container images | Fail immediately |
| **security** | Image vulnerability scan (`trivy`) | Fail on CRITICAL |
| **smoke** | Start Compose stack, health check, basic HTTP | Fail immediately |
| **publish** | Push images to registry | On main/tag only |
| **deploy** | Deploy to environment | On main/tag only |
| **notify** | Slack/email on failure | Always (if: always) |

---

## GitHub Actions — Canonical Symfony Pipeline

```yaml
# .github/workflows/ci.yml
name: CI

on:
  push:
    branches: [main, develop]
  pull_request:
    branches: [main, develop]

env:
  REGISTRY: ghcr.io
  IMAGE_NAME: ${{ github.repository }}

jobs:
  lint:
    name: Lint
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: Lint Dockerfile
        uses: hadolint/hadolint-action@v3.1.0
        with:
          dockerfile: docker/php/Dockerfile
          failure-threshold: warning

      - name: Validate docker-compose
        run: docker compose config -q

      - name: PHP CS Fixer (dry run)
        uses: docker://ghcr.io/php-cs-fixer/php-cs-fixer:3
        with:
          args: fix --dry-run --diff

      - name: PHPStan
        run: docker compose run --rm app vendor/bin/phpstan analyse

  test:
    name: Test
    runs-on: ubuntu-latest
    needs: lint
    steps:
      - uses: actions/checkout@v4

      - name: Copy env file
        run: cp .env.example .env.test

      - name: Start test services
        run: docker compose -f docker-compose.test.yml up -d --wait --timeout 120

      - name: Run PHPUnit
        run: |
          docker compose -f docker-compose.test.yml run --rm app \
            php bin/phpunit --coverage-clover coverage.xml

      - name: Upload coverage
        uses: codecov/codecov-action@v4
        with:
          files: coverage.xml

      - name: Teardown
        if: always()
        run: docker compose -f docker-compose.test.yml down -v

  build:
    name: Build image
    runs-on: ubuntu-latest
    needs: test
    permissions:
      contents: read
      packages: write
    outputs:
      image: ${{ steps.meta.outputs.tags }}
      digest: ${{ steps.build.outputs.digest }}
    steps:
      - uses: actions/checkout@v4

      - name: Set up Docker Buildx
        uses: docker/setup-buildx-action@v3

      - name: Log in to GHCR
        uses: docker/login-action@v3
        with:
          registry: ${{ env.REGISTRY }}
          username: ${{ github.actor }}
          password: ${{ secrets.GITHUB_TOKEN }}

      - name: Extract metadata
        id: meta
        uses: docker/metadata-action@v5
        with:
          images: ${{ env.REGISTRY }}/${{ env.IMAGE_NAME }}
          tags: |
            type=ref,event=branch
            type=ref,event=pr
            type=semver,pattern={{version}}
            type=sha,prefix=sha-

      - name: Build and push
        id: build
        uses: docker/build-push-action@v5
        with:
          context: .
          file: docker/php/Dockerfile
          target: production
          push: ${{ github.event_name != 'pull_request' }}
          tags: ${{ steps.meta.outputs.tags }}
          labels: ${{ steps.meta.outputs.labels }}
          cache-from: type=gha
          cache-to: type=gha,mode=max

  security:
    name: Security scan
    runs-on: ubuntu-latest
    needs: build
    if: github.event_name != 'pull_request'
    steps:
      - name: Scan image with Trivy
        uses: aquasecurity/trivy-action@master
        with:
          image-ref: ${{ needs.build.outputs.image }}
          format: sarif
          output: trivy-results.sarif
          severity: CRITICAL,HIGH
          exit-code: 1    # fail on CRITICAL

      - name: Upload Trivy results to GitHub Security
        uses: github/codeql-action/upload-sarif@v3
        if: always()
        with:
          sarif_file: trivy-results.sarif

  smoke:
    name: Smoke test
    runs-on: ubuntu-latest
    needs: build
    steps:
      - uses: actions/checkout@v4

      - name: Copy env file
        run: cp .env.example .env

      - name: Start full stack
        run: docker compose up -d --wait --timeout 120

      - name: Smoke test HTTP
        run: |
          curl -sf http://localhost/healthz
          curl -sf http://localhost/api/ping

      - name: Observability check
        run: |
          curl -sf http://localhost:9100/metrics | grep -q 'http_requests_total'

      - name: Teardown
        if: always()
        run: docker compose down -v

  deploy-staging:
    name: Deploy to staging
    runs-on: ubuntu-latest
    needs: [smoke, security]
    if: github.ref == 'refs/heads/main'
    environment:
      name: staging
      url: https://staging.example.com
    steps:
      - uses: actions/checkout@v4

      - name: Deploy via SSH
        uses: appleboy/ssh-action@v1
        with:
          host: ${{ secrets.STAGING_HOST }}
          username: ${{ secrets.STAGING_USER }}
          key: ${{ secrets.STAGING_SSH_KEY }}
          script: |
            cd /opt/myapp
            git pull
            docker compose pull
            docker compose up -d --no-deps app worker
            docker compose exec app php bin/console doctrine:migrations:migrate --no-interaction
```

---

## GitHub Actions — Reusable Workflow Pattern

```yaml
# .github/workflows/_reusable-docker-build.yml
name: Reusable Docker Build

on:
  workflow_call:
    inputs:
      image-name:
        required: true
        type: string
      dockerfile:
        required: false
        type: string
        default: Dockerfile
      build-target:
        required: false
        type: string
        default: production
    outputs:
      image-digest:
        value: ${{ jobs.build.outputs.digest }}
      image-tags:
        value: ${{ jobs.build.outputs.tags }}

jobs:
  build:
    runs-on: ubuntu-latest
    outputs:
      digest: ${{ steps.build.outputs.digest }}
      tags: ${{ steps.meta.outputs.tags }}
    steps:
      - uses: actions/checkout@v4
      - uses: docker/setup-buildx-action@v3
      - uses: docker/login-action@v3
        with:
          registry: ghcr.io
          username: ${{ github.actor }}
          password: ${{ secrets.GITHUB_TOKEN }}
      - id: meta
        uses: docker/metadata-action@v5
        with:
          images: ghcr.io/${{ inputs.image-name }}
      - id: build
        uses: docker/build-push-action@v5
        with:
          file: ${{ inputs.dockerfile }}
          target: ${{ inputs.build-target }}
          push: true
          tags: ${{ steps.meta.outputs.tags }}
          cache-from: type=gha
          cache-to: type=gha,mode=max
```

---

## GitHub Actions — Cache Strategies

```yaml
# Composer cache
- name: Get Composer cache directory
  id: composer-cache
  run: echo "dir=$(composer config cache-files-dir)" >> $GITHUB_OUTPUT

- name: Cache Composer dependencies
  uses: actions/cache@v4
  with:
    path: ${{ steps.composer-cache.outputs.dir }}
    key: ${{ runner.os }}-composer-${{ hashFiles('**/composer.lock') }}
    restore-keys: ${{ runner.os }}-composer-

# Node.js / npm cache
- uses: actions/setup-node@v4
  with:
    node-version: 20
    cache: npm

# Docker layer cache (BuildKit GHA cache)
- uses: docker/build-push-action@v5
  with:
    cache-from: type=gha
    cache-to: type=gha,mode=max
```

---

## GitHub Actions — Matrix Build

```yaml
# Run tests against multiple PHP versions
strategy:
  matrix:
    php-version: ["8.2", "8.3"]
    mariadb-version: ["10.11", "11.4"]
  fail-fast: false

steps:
  - name: Run tests
    env:
      PHP_VERSION: ${{ matrix.php-version }}
      MARIADB_VERSION: ${{ matrix.mariadb-version }}
    run: docker compose -f docker-compose.test.yml up --build --exit-code-from app
```

---

## GitLab CI — Canonical Symfony Pipeline

```yaml
# .gitlab-ci.yml
stages:
  - lint
  - test
  - build
  - security
  - smoke
  - deploy

variables:
  DOCKER_DRIVER: overlay2
  DOCKER_BUILDKIT: "1"
  IMAGE_TAG: $CI_REGISTRY_IMAGE:$CI_COMMIT_SHA
  LATEST_TAG: $CI_REGISTRY_IMAGE:latest

default:
  image: docker:26-cli
  services:
    - docker:26-dind
  before_script:
    - docker login -u $CI_REGISTRY_USER -p $CI_REGISTRY_PASSWORD $CI_REGISTRY

# ---- Lint ----
lint:dockerfile:
  stage: lint
  image: hadolint/hadolint:latest-debian
  script:
    - hadolint docker/php/Dockerfile
  rules:
    - changes:
        - docker/**/*
        - .gitlab-ci.yml

lint:phpstan:
  stage: lint
  script:
    - cp .env.example .env
    - docker compose run --rm app vendor/bin/phpstan analyse --no-progress
  rules:
    - changes:
        - "**/*.php"

# ---- Test ----
test:phpunit:
  stage: test
  script:
    - cp .env.example .env.test
    - docker compose -f docker-compose.test.yml up -d --wait
    - docker compose -f docker-compose.test.yml run --rm app
        php bin/phpunit --coverage-text --log-junit junit.xml
  after_script:
    - docker compose -f docker-compose.test.yml down -v
  artifacts:
    reports:
      junit: junit.xml
    when: always
    expire_in: 1 week
  coverage: '/Lines:\s+(\d+\.\d+)%/'

# ---- Build ----
build:image:
  stage: build
  script:
    - docker build
        --target production
        --cache-from $LATEST_TAG
        --build-arg BUILDKIT_INLINE_CACHE=1
        -t $IMAGE_TAG
        -t $LATEST_TAG
        -f docker/php/Dockerfile .
    - docker push $IMAGE_TAG
    - docker push $LATEST_TAG
  only:
    - main
    - tags

# ---- Security ----
security:trivy:
  stage: security
  image:
    name: aquasec/trivy:latest
    entrypoint: [""]
  script:
    - trivy image
        --exit-code 1
        --severity CRITICAL
        --format sarif
        --output trivy-results.sarif
        $IMAGE_TAG
  artifacts:
    reports:
      sast: trivy-results.sarif
    when: always
  only:
    - main
    - tags

# ---- Smoke ----
smoke:test:
  stage: smoke
  script:
    - cp .env.example .env
    - docker compose up -d --wait --timeout 120
    - curl -sf http://localhost/healthz
    - curl -sf http://localhost:9100/metrics | grep -q http_requests_total
  after_script:
    - docker compose down -v
  only:
    - main
    - tags

# ---- Deploy ----
.deploy-template: &deploy-template
  stage: deploy
  image: alpine:3.19
  before_script:
    - apk add --no-cache openssh-client
    - eval $(ssh-agent -s)
    - echo "$DEPLOY_SSH_KEY" | ssh-add -
    - mkdir -p ~/.ssh && chmod 700 ~/.ssh
    - ssh-keyscan $DEPLOY_HOST >> ~/.ssh/known_hosts

deploy:staging:
  <<: *deploy-template
  environment:
    name: staging
    url: https://staging.example.com
  script:
    - |
      ssh $DEPLOY_USER@$DEPLOY_HOST "
        cd /opt/myapp &&
        git pull &&
        docker compose pull &&
        docker compose up -d --no-deps app worker &&
        docker compose exec -T app php bin/console doctrine:migrations:migrate --no-interaction
      "
  only:
    - main

deploy:production:
  <<: *deploy-template
  environment:
    name: production
    url: https://app.example.com
  script:
    - |
      ssh $DEPLOY_USER@$PROD_HOST "
        cd /opt/myapp &&
        git pull &&
        docker compose pull &&
        docker compose up -d --no-deps app worker &&
        docker compose exec -T app php bin/console doctrine:migrations:migrate --no-interaction
      "
  when: manual     # requires explicit approval
  only:
    - tags

# ---- Notify ----
notify:failure:
  stage: .post
  script:
    - |
      curl -sf -X POST -H 'Content-type: application/json' \
        --data "{\"text\":\"Pipeline failed on ${CI_COMMIT_REF_NAME}: ${CI_PIPELINE_URL}\"}" \
        $SLACK_WEBHOOK_URL
  when: on_failure
```

---

## GitLab CI — Caching Strategy

```yaml
# Global cache for Composer
cache:
  key:
    files:
      - composer.lock
  paths:
    - vendor/

# Per-job Docker layer cache via registry
build:image:
  script:
    - docker build
        --cache-from $CI_REGISTRY_IMAGE:cache
        --build-arg BUILDKIT_INLINE_CACHE=1
        -t $IMAGE_TAG
        --target production .
    - docker push $IMAGE_TAG
    # Push cache layer
    - docker build
        --target base
        -t $CI_REGISTRY_IMAGE:cache .
    - docker push $CI_REGISTRY_IMAGE:cache
```

---

## Deployment Patterns

### Rolling restart (Docker Compose, zero-downtime)

```bash
# Rolling update of app service without downtime
docker compose pull app
docker compose up -d --no-deps --scale app=2 app   # spin up new instance
sleep 10                                            # wait for health check
docker compose up -d --no-deps --scale app=1 app   # remove old instance
```

### Blue-green deployment (Docker Compose)

```bash
# Switch between blue and green stacks using Traefik labels
export ACTIVE_COLOR=green
docker compose -f docker-compose.${ACTIVE_COLOR}.yml up -d --build
# Update Traefik routing rule to point at new stack
# Then stop old stack after traffic drains
```

### Database migration safety gate

```bash
# Always run migrations before deploying new code
# Migrations must be backward-compatible (old code runs against new schema)
docker compose exec app php bin/console doctrine:migrations:migrate --no-interaction
# Verify schema matches expected state
docker compose exec app php bin/console doctrine:schema:validate
```

---

## Environment Variable Conventions

| Variable | Purpose | Example |
|---|---|---|
| `APP_ENV` | Runtime environment | `dev`, `test`, `staging`, `prod` |
| `APP_SECRET` | Symfony app secret | 32+ char random string |
| `APP_VERSION` | Image / deploy version | `1.4.2` or `sha-abc123` |
| `DATABASE_URL` | Full DSN for ORM | `mysql://user:pass@host:3306/db?serverVersion=mariadb-10.11.0` |
| `REDIS_URL` | Redis connection string | `redis://redis:6379` |
| `COMPOSE_PROJECT_NAME` | Docker namespace | `myapp` |
| `DEPLOY_HOST` | Deployment target SSH host | `192.168.1.10` |
| `OTEL_SERVICE_NAME` | OpenTelemetry service label | `api`, `worker` |
