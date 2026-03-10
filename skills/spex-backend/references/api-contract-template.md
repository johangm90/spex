# API Contract Template — spex-backend

Use this template when producing an `api_contract` artifact for a new slice.

---

## Artifact Front-Matter

Every API contract stored in MCP must begin with a front-matter block describing the artifact:

```yaml
# artifact: PROJ-API-NNN
# slice:    SLICE-NNN
# task:     T0NN-N
# agent:    spex-backend
# status:   draft | review | approved
# version:  1.0.0
```

---

## MCP Storage Pattern

1. Register the artifact so it appears in the artifact index:

```js
artifact_register(
  id="PROJ-API-NNN",
  spec="SLICE-NNN",
  task="T0NN-N",
  agent="spex-backend",
  type="api_contract",
  path="mcp:api/PROJ-API-NNN",
  description="OpenAPI 3.1 contract for <feature>."
)
```

2. Store the full spec content in agent memory:

```js
memory_set(
  agent="spex-backend",
  key="artifact_PROJ-API-NNN",
  type="architecture",
  value="<full OpenAPI YAML/JSON string>"
)
```

---

## OpenAPI 3.1 Skeleton

```yaml
openapi: 3.1.0
info:
  title: <Feature Name> API
  version: 1.0.0
  description: |
    API contract for SLICE-NNN — <one-line description>.

servers:
  - url: /api/v1

paths:
  /resource:
    post:
      operationId: createResource
      summary: Create a new resource
      tags: [Resource]
      requestBody:
        required: true
        content:
          application/json:
            schema:
              $ref: '#/components/schemas/CreateResourceRequest'
      responses:
        '201':
          description: Resource created
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/ResourceResponse'
        '400':
          $ref: '#/components/responses/ValidationError'
        '409':
          $ref: '#/components/responses/ConflictError'
        '422':
          $ref: '#/components/responses/UnprocessableEntity'

components:
  schemas:
    CreateResourceRequest:
      type: object
      required: [field1, field2]
      properties:
        field1:
          type: string
          description: Primary identifier supplied by caller
        field2:
          type: string
          description: Secondary value

    ResourceResponse:
      type: object
      properties:
        id:
          type: string
          format: uuid
        field1:
          type: string
        field2:
          type: string
        createdAt:
          type: string
          format: date-time

    ErrorResponse:
      type: object
      properties:
        code:
          type: string
        message:
          type: string
        details:
          type: array
          items:
            type: object

  responses:
    ValidationError:
      description: Request body failed schema validation
      content:
        application/json:
          schema:
            $ref: '#/components/schemas/ErrorResponse'
    ConflictError:
      description: Resource already exists (duplicate idempotency key)
      content:
        application/json:
          schema:
            $ref: '#/components/schemas/ErrorResponse'
    UnprocessableEntity:
      description: Request is well-formed but violates business rules
      content:
        application/json:
          schema:
            $ref: '#/components/schemas/ErrorResponse'
```

---

## Rules for API Contract Authors

| Rule | Rationale |
|------|-----------|
| Every mutating endpoint must return `409` for duplicate idempotency key | Idempotency enforcement |
| Monetary fields must use `type: string` or `type: integer` (cents) — never `number` | No float money |
| All IDs must be `format: uuid` unless the slice spec explicitly states otherwise | Consistency |
| Pagination responses must include `total`, `page`, and `pageSize` fields | Discoverability |
| Authentication scheme must be declared under `components/securitySchemes` if the endpoint is protected | Security |
