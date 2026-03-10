# Kubernetes — spex-devops

## When to Use Kubernetes

Use Kubernetes when you need **at least two** of the following:

- Horizontal auto-scaling (HPA) based on CPU/memory or custom metrics
- Multiple replicas with zero-downtime rolling updates
- Multiple namespaces for environment isolation (staging, prod in same cluster)
- Self-healing (automatic pod restart on crash)
- Fine-grained resource quotas and limits per workload
- Node affinity / tolerations for heterogeneous node pools

For single-server deployments, prefer Docker Compose (see `references/infra-patterns.md`).

**Recommended distributions:**
| Distribution | When to use |
|---|---|
| **k3s** | Single server or small cluster (1–3 nodes); minimal resource overhead |
| **EKS (AWS)** | AWS-native; managed control plane |
| **GKE (Google)** | GCP-native; Autopilot mode for fully managed nodes |
| **AKS (Azure)** | Azure-native; free control plane |
| **kind / minikube** | Local development and CI only; not for production |

---

## Namespace Strategy

```yaml
# Always use namespaces to separate environments
# staging namespace
apiVersion: v1
kind: Namespace
metadata:
  name: myapp-staging
  labels:
    project: myapp
    env: staging

---
# production namespace
apiVersion: v1
kind: Namespace
metadata:
  name: myapp-production
  labels:
    project: myapp
    env: production
```

---

## Deployment

```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: app
  namespace: myapp-production
  labels:
    project: myapp
    env: production
    component: app
spec:
  replicas: 3
  selector:
    matchLabels:
      app: myapp-app
  strategy:
    type: RollingUpdate
    rollingUpdate:
      maxSurge: 1        # allow 1 extra pod during rollout
      maxUnavailable: 0  # never kill a pod before a new one is ready
  template:
    metadata:
      labels:
        app: myapp-app
        version: "1.4.2"
    spec:
      serviceAccountName: myapp-app

      # Security context — run as non-root
      securityContext:
        runAsNonRoot: true
        runAsUser: 1000
        fsGroup: 1000

      containers:
        - name: app
          image: ghcr.io/org/myapp:sha-abc123   # always pin; never :latest
          imagePullPolicy: IfNotPresent

          ports:
            - containerPort: 9000
              name: fpm

          envFrom:
            - configMapRef:
                name: myapp-config
            - secretRef:
                name: myapp-secrets

          resources:
            requests:
              cpu: "100m"
              memory: "128Mi"
            limits:
              cpu: "500m"
              memory: "512Mi"

          readinessProbe:
            httpGet:
              path: /healthz
              port: 9000
            initialDelaySeconds: 10
            periodSeconds: 5
            failureThreshold: 3

          livenessProbe:
            httpGet:
              path: /healthz
              port: 9000
            initialDelaySeconds: 30
            periodSeconds: 10
            failureThreshold: 5

          startupProbe:
            httpGet:
              path: /healthz
              port: 9000
            failureThreshold: 30
            periodSeconds: 10

          # Restrict container capabilities
          securityContext:
            allowPrivilegeEscalation: false
            readOnlyRootFilesystem: true
            capabilities:
              drop: ["ALL"]

          volumeMounts:
            - name: tmp
              mountPath: /tmp
            - name: var-cache
              mountPath: /var/www/html/var/cache

      volumes:
        - name: tmp
          emptyDir: {}
        - name: var-cache
          emptyDir: {}

      # Spread pods across nodes for HA
      topologySpreadConstraints:
        - maxSkew: 1
          topologyKey: kubernetes.io/hostname
          whenUnsatisfiable: DoNotSchedule
          labelSelector:
            matchLabels:
              app: myapp-app

      # Graceful shutdown
      terminationGracePeriodSeconds: 30
```

---

## Service

```yaml
# ClusterIP — internal-only access
apiVersion: v1
kind: Service
metadata:
  name: app
  namespace: myapp-production
  labels:
    project: myapp
    component: app
spec:
  selector:
    app: myapp-app
  ports:
    - name: fpm
      port: 9000
      targetPort: 9000
  type: ClusterIP

---
# For HTTP apps (non-FPM), use port 80 → 3000
apiVersion: v1
kind: Service
metadata:
  name: api
  namespace: myapp-production
spec:
  selector:
    app: myapp-api
  ports:
    - name: http
      port: 80
      targetPort: 3000
  type: ClusterIP
```

---

## Ingress (nginx ingress controller)

```yaml
apiVersion: networking.k8s.io/v1
kind: Ingress
metadata:
  name: myapp
  namespace: myapp-production
  annotations:
    # nginx ingress controller
    nginx.ingress.kubernetes.io/proxy-body-size: "10m"
    nginx.ingress.kubernetes.io/proxy-read-timeout: "30"
    nginx.ingress.kubernetes.io/enable-cors: "true"
    # cert-manager for automatic TLS
    cert-manager.io/cluster-issuer: letsencrypt-prod
    # Security headers
    nginx.ingress.kubernetes.io/configuration-snippet: |
      more_set_headers "Strict-Transport-Security: max-age=31536000; includeSubDomains; preload";
      more_set_headers "X-Content-Type-Options: nosniff";
      more_set_headers "X-Frame-Options: DENY";
spec:
  ingressClassName: nginx
  tls:
    - hosts:
        - app.example.com
      secretName: myapp-tls
  rules:
    - host: app.example.com
      http:
        paths:
          - path: /
            pathType: Prefix
            backend:
              service:
                name: app
                port:
                  name: http
```

---

## ConfigMap

```yaml
apiVersion: v1
kind: ConfigMap
metadata:
  name: myapp-config
  namespace: myapp-production
data:
  APP_ENV: "prod"
  APP_DEBUG: "0"
  DATABASE_HOST: "mariadb.myapp-production.svc.cluster.local"
  DATABASE_PORT: "3306"
  DATABASE_NAME: "myapp"
  REDIS_HOST: "redis.myapp-production.svc.cluster.local"
  OTEL_SERVICE_NAME: "app"
  OTEL_EXPORTER_OTLP_ENDPOINT: "http://otel-collector:4317"
```

---

## Secret

```yaml
# NEVER commit real Secret manifests.
# Use sealed-secrets, external-secrets, or a vault operator instead.
# This is for illustration only — inject via CI/CD pipeline.
apiVersion: v1
kind: Secret
metadata:
  name: myapp-secrets
  namespace: myapp-production
type: Opaque
stringData:   # use stringData so values are not pre-encoded in source
  APP_SECRET: "${APP_SECRET}"
  DATABASE_URL: "mysql://${DB_USER}:${DB_PASSWORD}@mariadb:3306/myapp?serverVersion=mariadb-10.11.0&charset=utf8mb4"
  DATABASE_USER: "${DB_USER}"
  DATABASE_PASSWORD: "${DB_PASSWORD}"
```

### External Secrets Operator (recommended for production)

```yaml
apiVersion: external-secrets.io/v1beta1
kind: ExternalSecret
metadata:
  name: myapp-secrets
  namespace: myapp-production
spec:
  refreshInterval: 1h
  secretStoreRef:
    name: vault-backend       # or aws-ssm, gcp-sm, etc.
    kind: ClusterSecretStore
  target:
    name: myapp-secrets
    creationPolicy: Owner
  data:
    - secretKey: APP_SECRET
      remoteRef:
        key: myapp/production
        property: app_secret
    - secretKey: DATABASE_PASSWORD
      remoteRef:
        key: myapp/production
        property: db_password
```

---

## Horizontal Pod Autoscaler (HPA)

```yaml
apiVersion: autoscaling/v2
kind: HorizontalPodAutoscaler
metadata:
  name: app
  namespace: myapp-production
spec:
  scaleTargetRef:
    apiVersion: apps/v1
    kind: Deployment
    name: app
  minReplicas: 2
  maxReplicas: 10
  metrics:
    - type: Resource
      resource:
        name: cpu
        target:
          type: Utilization
          averageUtilization: 70      # scale up when avg CPU > 70%
    - type: Resource
      resource:
        name: memory
        target:
          type: Utilization
          averageUtilization: 80
  behavior:
    scaleUp:
      stabilizationWindowSeconds: 60
      policies:
        - type: Pods
          value: 2
          periodSeconds: 60
    scaleDown:
      stabilizationWindowSeconds: 300   # wait 5 min before scaling down
      policies:
        - type: Pods
          value: 1
          periodSeconds: 120
```

---

## PodDisruptionBudget

```yaml
# Ensure at least 1 pod is always available during node drain / upgrades
apiVersion: policy/v1
kind: PodDisruptionBudget
metadata:
  name: app-pdb
  namespace: myapp-production
spec:
  minAvailable: 1
  selector:
    matchLabels:
      app: myapp-app
```

---

## ServiceAccount + RBAC (least privilege)

```yaml
apiVersion: v1
kind: ServiceAccount
metadata:
  name: myapp-app
  namespace: myapp-production
automountServiceAccountToken: false   # disable unless needed

---
# If the app needs to read ConfigMaps (e.g. for leader election):
apiVersion: rbac.authorization.k8s.io/v1
kind: Role
metadata:
  name: myapp-app
  namespace: myapp-production
rules:
  - apiGroups: [""]
    resources: ["configmaps"]
    verbs: ["get", "list", "watch"]

---
apiVersion: rbac.authorization.k8s.io/v1
kind: RoleBinding
metadata:
  name: myapp-app
  namespace: myapp-production
subjects:
  - kind: ServiceAccount
    name: myapp-app
roleRef:
  kind: Role
  name: myapp-app
  apiGroup: rbac.authorization.k8s.io
```

---

## Resource Limits Reference

| Workload type | CPU request | CPU limit | Memory request | Memory limit |
|---|---|---|---|---|
| PHP-FPM (light) | 100m | 500m | 128Mi | 512Mi |
| PHP-FPM (heavy) | 250m | 1000m | 256Mi | 1Gi |
| Node.js API | 100m | 500m | 128Mi | 512Mi |
| Background worker | 250m | 1000m | 256Mi | 1Gi |
| MariaDB (small) | 250m | 1000m | 512Mi | 2Gi |
| Redis (small) | 50m | 250m | 64Mi | 256Mi |

Rules:
- Always set both `requests` and `limits`
- CPU limit ≥ 2× CPU request (burst-friendly)
- Memory limit ≈ memory request (OOMKill is hard to debug; size accurately)

---

## Init Container for Migrations

```yaml
# Run migrations before the app container starts
initContainers:
  - name: migrations
    image: ghcr.io/org/myapp:sha-abc123
    command: ["php", "bin/console", "doctrine:migrations:migrate", "--no-interaction"]
    envFrom:
      - configMapRef:
          name: myapp-config
      - secretRef:
          name: myapp-secrets
    resources:
      requests:
        cpu: "100m"
        memory: "128Mi"
      limits:
        cpu: "500m"
        memory: "256Mi"
```

---

## Helm Chart Structure (for complex projects)

```
charts/myapp/
├── Chart.yaml
├── values.yaml                # defaults
├── values.staging.yaml        # staging overrides
├── values.production.yaml     # production overrides
└── templates/
    ├── deployment.yaml
    ├── service.yaml
    ├── ingress.yaml
    ├── configmap.yaml
    ├── hpa.yaml
    ├── pdb.yaml
    ├── serviceaccount.yaml
    └── _helpers.tpl
```

```yaml
# Chart.yaml
apiVersion: v2
name: myapp
description: My Application Helm Chart
type: application
version: 0.1.0
appVersion: "1.4.2"

# values.yaml (key entries)
replicaCount: 2
image:
  repository: ghcr.io/org/myapp
  tag: ""       # overridden at deploy time with --set image.tag=sha-abc123
  pullPolicy: IfNotPresent
ingress:
  enabled: true
  host: app.example.com
  tls: true
resources:
  requests:
    cpu: 100m
    memory: 128Mi
  limits:
    cpu: 500m
    memory: 512Mi
autoscaling:
  enabled: true
  minReplicas: 2
  maxReplicas: 10
  targetCPUUtilizationPercentage: 70
```

### Deploy command

```bash
helm upgrade --install myapp ./charts/myapp \
  --namespace myapp-production \
  --create-namespace \
  -f charts/myapp/values.production.yaml \
  --set image.tag=$IMAGE_TAG \
  --atomic \           # rollback automatically on failure
  --timeout 5m \
  --wait
```

---

## Kubernetes Security Checklist

- [ ] All pods run as non-root (`runAsNonRoot: true`)
- [ ] `allowPrivilegeEscalation: false` on all containers
- [ ] `readOnlyRootFilesystem: true` (use `emptyDir` for writable paths)
- [ ] `capabilities: drop: ["ALL"]` on all containers
- [ ] No `hostNetwork`, `hostPID`, `hostIPC` unless explicitly required
- [ ] `automountServiceAccountToken: false` on ServiceAccounts unless needed
- [ ] Secrets managed via External Secrets Operator or sealed-secrets
- [ ] Network Policies restricting ingress/egress to known peers
- [ ] Image tags pinned (no `:latest`)
- [ ] Images scanned with Trivy in CI before deploy
- [ ] Resource `requests` and `limits` set on all containers
- [ ] PodDisruptionBudget defined for all critical Deployments
- [ ] Liveness, readiness, and startup probes defined
