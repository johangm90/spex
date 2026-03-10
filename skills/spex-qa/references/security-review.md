# Security Review — spex-qa Reference

STRIDE threat modeling, OWASP Top 10 2021 test cases, auth/authz checklist, input validation, and dependency audit commands.

---

## When to Produce a Full STRIDE Model

Run the full STRIDE model when the slice:
- Introduces or modifies **authentication or authorisation** logic
- Adds a **new external-facing endpoint or service**
- Handles **sensitive data** (PII, payment, health, credentials)
- Introduces a **new infrastructure component** (queue, cache, external API)
- Changes **trust boundaries** between services

For slices that only modify existing CRUD logic, the security review checklist (below) is sufficient.

---

## STRIDE Threat Model Template

```
Slice: SLICE-NNN
Date: <ISO date>
Reviewers: spex-qa

## Data Flow Diagram (text)
[Browser] → HTTPS → [Caddy proxy] → [App: PHP-FPM] → [MariaDB]
                                  ↓
                              [Redis cache]
                                  ↓
                          [Background worker] → [External payment API]

## Trust Boundaries
- Internet ↔ Caddy proxy (TLS enforced)
- Proxy ↔ App (internal Docker network, no TLS)
- App ↔ MariaDB (internal network, authenticated)
- App ↔ External API (HTTPS, API key)
```

| Threat | Category | Asset | Threat Description | Existing Control | Residual Risk | Test case ID |
|---|---|---|---|---|---|---|
| Spoof user identity | **S**poofing | JWT token | Attacker forges or replays a JWT | Short TTL (15m), HS256 signature | Low | TC-032 |
| Tamper with order amount | **T**ampering | Order total | Client sends manipulated price in payload | Price computed server-side only | Low | TC-034 |
| Repudiate failed payment | **R**epudiation | Payment event | User denies making a charge | Immutable payment log with user ID + timestamp | Low | — |
| Leak PII in error response | **I**nformation Disclosure | Error body | Stack trace exposes DB schema or env vars | `APP_DEBUG=false` in prod; custom error handler | Medium | TC-035 |
| Flood login endpoint | **D**enial of Service | Auth endpoint | Brute-force or credential stuffing | Rate limiting (60 req/min per IP) | Medium | TC-040 |
| Privilege escalation via IDOR | **E**levation of Privilege | Order resource | User accesses another user's order by ID | Ownership check in query; returns 403 | Low | TC-031 |

### STRIDE category quick reference

| Letter | Category | Key question |
|---|---|---|
| S | Spoofing | Can an attacker impersonate a user or service? |
| T | Tampering | Can an attacker modify data in transit or at rest? |
| R | Repudiation | Can a user deny an action without proof? |
| I | Information Disclosure | Can an attacker read data they shouldn't? |
| D | Denial of Service | Can an attacker make the system unavailable? |
| E | Elevation of Privilege | Can an attacker gain permissions they shouldn't have? |

---

## OWASP Top 10 2021 — Test Case Mapping

For each item: threat description, how to test, expected pass criterion.

### A01 — Broken Access Control

| Test | How to test | Pass criterion |
|---|---|---|
| IDOR — access another user's resource | Authenticated as user A, request resource owned by user B | 403 or 404; resource data not returned |
| Privilege escalation via role manipulation | User sends `role: "admin"` in POST body | Field ignored; role unchanged in DB |
| Force-browse to admin URL | Authenticated as `ROLE_USER`, GET `/admin/users` | 403 |
| JWT missing scope | Remove a required claim from JWT | 401 or 403 |

### A02 — Cryptographic Failures

| Test | How to test | Pass criterion |
|---|---|---|
| Sensitive data in HTTP (non-TLS) | Check response headers for `Strict-Transport-Security` | Header present; max-age ≥ 31536000 |
| Password stored in plaintext | Inspect DB after registration | Column contains bcrypt/argon2 hash, never plaintext |
| Sensitive fields in logs | Trigger a request with `Authorization` header; inspect logs | No token, password, or PII in log output |
| Weak TLS config | Run `testssl.sh` or SSL Labs scan | No TLS < 1.2; no weak ciphers (RC4, DES) |

### A03 — Injection

| Test | How to test | Pass criterion |
|---|---|---|
| SQL injection — query param | `GET /api/orders?search=1' OR '1'='1` | 400 validation error or safe empty result; no DB error |
| SQL injection — JSON body | `{"name": "'; DROP TABLE orders; --"}` | Stored as literal string; no SQL executed |
| Command injection | `{"filename": "file.txt; rm -rf /"}` | 400 or field rejected; no command executed |
| LDAP injection | `{"username": "*)(uid=*))(|(uid=*"}` | Input rejected; no auth bypass |
| XSS — stored | POST `<script>alert(1)</script>` in a text field; retrieve and render | Script tag escaped in response; not executed |
| XSS — reflected | `GET /search?q=<script>alert(1)</script>` | Output escaped; no script tag in HTML response |

### A04 — Insecure Design

| Test | How to test | Pass criterion |
|---|---|---|
| Business logic bypass | Skip mandatory steps (e.g., go directly to checkout without adding items) | System rejects or enforces prerequisite state |
| Negative quantity order | `{"quantity": -5}` | 400 validation error |
| Replay attack on token | Reuse a single-use token (password reset, email verification) | Token rejected on second use |

### A05 — Security Misconfiguration

| Test | How to test | Pass criterion |
|---|---|---|
| Default credentials | Try `admin/admin`, `root/root`, etc. | All default credentials changed; access denied |
| Verbose error messages | Trigger a 500 in production | No stack trace, SQL, or env var in response body |
| Directory listing | GET `/` on file server paths | No directory listing; 403 or 404 |
| Unnecessary HTTP methods | `TRACE /api/orders` | 405 Method Not Allowed |
| Missing security headers | Check response headers | `X-Content-Type-Options`, `X-Frame-Options`, `CSP` present |

### A06 — Vulnerable and Outdated Components

| Test | How to test | Pass criterion |
|---|---|---|
| Known CVEs in dependencies | `composer audit` / `npm audit` / `trivy image` | No CRITICAL; HIGH reviewed and mitigated |
| Outdated base image | `trivy image --severity HIGH,CRITICAL <image>` | No unpatched CRITICAL OS packages |

### A07 — Identification and Authentication Failures

| Test | How to test | Pass criterion |
|---|---|---|
| Brute-force login | 100 rapid POST `/api/auth` attempts | Rate limited after N attempts (429 Too Many Requests) |
| Weak password accepted | Register with `password=123` | Rejected; minimum complexity enforced |
| Session fixation | Use pre-auth session ID post-login | New session ID issued after login |
| JWT algorithm confusion | Send `alg: none` JWT | Token rejected; 401 |
| Expired JWT accepted | Use JWT with past `exp` claim | Rejected; 401 |

### A08 — Software and Data Integrity Failures

| Test | How to test | Pass criterion |
|---|---|---|
| Unsigned/unverified dependency | Check `composer.lock` / `package-lock.json` is committed | Lock file present and committed |
| CI pipeline tampering | Review GitHub Actions permissions | Minimum-privilege tokens; no `write-all` permission |

### A09 — Security Logging and Monitoring Failures

| Test | How to test | Pass criterion |
|---|---|---|
| Auth failures logged | Attempt login with wrong password; check logs | Event logged with timestamp, IP, username (not password) |
| No PII in logs | Make a request with `Authorization` header; inspect output | No token or password in any log line |
| Anomaly not detected | Simulate 100 failed logins from one IP | Alert or rate limit triggered within the window |

### A10 — Server-Side Request Forgery (SSRF)

| Test | How to test | Pass criterion |
|---|---|---|
| SSRF via URL parameter | `{"webhook_url": "http://169.254.169.254/latest/meta-data/"}` | URL rejected; allowlist enforced |
| SSRF via redirect | Point to a URL that redirects to internal IP | Final destination validated; internal IPs blocked |

---

## Authentication & Authorisation Checklist

### Authentication

- [ ] All protected endpoints return 401 when `Authorization` header is absent
- [ ] All protected endpoints return 401 when JWT is expired
- [ ] All protected endpoints return 401 when JWT signature is invalid (`alg: none`, wrong secret)
- [ ] Login endpoint is rate-limited (429 after N failures per IP)
- [ ] Password reset and email verification tokens are single-use and expire
- [ ] Passwords stored with bcrypt or argon2 (never MD5, SHA-1, or plaintext)
- [ ] Refresh tokens are rotated on each use (rotation + family invalidation)

### Authorisation

- [ ] Every resource endpoint checks ownership or role at the **resource level** (not just route level)
- [ ] `ROLE_USER` cannot access `ROLE_ADMIN` endpoints (403 test in place)
- [ ] Cross-tenant access is prevented (user from tenant A cannot read tenant B's data)
- [ ] Mass assignment is blocked — forbidden fields (role, id, tenant_id) ignored on write
- [ ] Soft-deleted resources return 404, not 403 (no information leakage about existence)

---

## Input Validation Checklist

- [ ] All query parameters validated (type, range, format) before use in queries
- [ ] All JSON body fields validated against schema before persistence
- [ ] File uploads restricted by type (MIME + magic bytes), size, and filename
- [ ] URL parameters (UUIDs, slugs) validated by format before DB lookup
- [ ] All DB queries use parameterized statements or ORM (no string concatenation)
- [ ] All output HTML-encoded before rendering (XSS prevention)
- [ ] Content-Security-Policy header set on all HTML responses
- [ ] `Content-Type` header validated on POST/PATCH requests (reject unexpected types)

---

## Dependency Audit Commands

```bash
# PHP / Composer
composer audit
# Lists packages with known CVEs from the Symfony security advisories database

# Node.js / npm
npm audit --audit-level=high
# Fails on HIGH or CRITICAL vulnerabilities

# Node.js / yarn
yarn npm audit --severity high

# Docker images — Trivy
trivy image --severity HIGH,CRITICAL ghcr.io/org/myapp:sha-abc123

# File system scan (in CI before build)
trivy fs --severity HIGH,CRITICAL .

# Secrets detection
trivy fs --scanners secret .
# or
gitleaks detect --source . --verbose
```

### CI gate snippet

```yaml
- name: Composer security audit
  run: docker compose run --rm app composer audit

- name: npm audit
  run: npm audit --audit-level=high

- name: Trivy image scan
  uses: aquasecurity/trivy-action@master
  with:
    image-ref: ${{ env.IMAGE_TAG }}
    severity: CRITICAL,HIGH
    exit-code: 1
```

---

## Security Headers Reference

Every HTTP response from the public proxy must include:

| Header | Required value | Purpose |
|---|---|---|
| `Strict-Transport-Security` | `max-age=31536000; includeSubDomains; preload` | Force HTTPS |
| `X-Content-Type-Options` | `nosniff` | Prevent MIME sniffing |
| `X-Frame-Options` | `DENY` | Prevent clickjacking |
| `Referrer-Policy` | `strict-origin-when-cross-origin` | Limit referrer leakage |
| `Content-Security-Policy` | `default-src 'self'; ...` | XSS mitigation |
| `Permissions-Policy` | `camera=(), microphone=(), geolocation=()` | Disable unused APIs |

Test with:
```bash
curl -I https://app.example.com | grep -E 'strict-transport|x-content|x-frame|referrer|content-security'
# or use securityheaders.com
```
