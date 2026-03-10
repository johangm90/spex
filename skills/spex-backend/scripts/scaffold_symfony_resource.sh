#!/usr/bin/env bash
# scaffold_symfony_resource.sh
# Generates boilerplate for a new API Platform resource in a Symfony project.
#
# Usage:
#   ./skills/spex-backend/scripts/scaffold_symfony_resource.sh <ResourceName> [namespace]
#
# Example:
#   ./skills/spex-backend/scripts/scaffold_symfony_resource.sh Order
#   ./skills/spex-backend/scripts/scaffold_symfony_resource.sh Product App\\Catalog
#
# Generates:
#   src/Domain/Model/<Resource>.php
#   src/Domain/Repository/<Resource>RepositoryInterface.php
#   src/Infrastructure/Persistence/Doctrine<Resource>Repository.php
#   src/ApiPlatform/Resource/<Resource>Resource.php
#   src/ApiPlatform/Processor/Create<Resource>Processor.php
#   src/ApiPlatform/Provider/<Resource>Provider.php
#   tests/Unit/Domain/<Resource>Test.php
#   tests/Api/<Resource>Test.php

set -euo pipefail

# ─── Args ────────────────────────────────────────────────────────────────────
RESOURCE="${1:-}"
if [[ -z "$RESOURCE" ]]; then
  echo "Usage: $0 <ResourceName> [BaseNamespace]"
  echo "Example: $0 Order"
  exit 1
fi

BASE_NS="${2:-App}"
RESOURCE_LOWER=$(echo "$RESOURCE" | tr '[:upper:]' '[:lower:]')
RESOURCE_PLURAL="${RESOURCE_LOWER}s"

echo "🏗  Scaffolding API Platform resource: $RESOURCE"
echo "   Base namespace : $BASE_NS"
echo ""

# ─── Helpers ─────────────────────────────────────────────────────────────────
write_file() {
  local path="$1"
  local content="$2"
  mkdir -p "$(dirname "$path")"
  if [[ -f "$path" ]]; then
    echo "  ⚠️  SKIP (exists): $path"
    return
  fi
  echo "$content" > "$path"
  echo "  ✅ Created: $path"
}

# ─── 1. Domain Entity ─────────────────────────────────────────────────────────
write_file "src/Domain/Model/${RESOURCE}.php" "<?php

declare(strict_types=1);

namespace ${BASE_NS}\\Domain\\Model;

use Doctrine\\ORM\\Mapping as ORM;

#[ORM\\Entity]
#[ORM\\Table(name: '${RESOURCE_PLURAL}')]
#[ORM\\HasLifecycleCallbacks]
class ${RESOURCE}
{
    #[ORM\\Id]
    #[ORM\\Column(type: 'uuid', unique: true)]
    private string \$id;

    // TODO: add domain fields here

    #[ORM\\Column]
    private \\DateTimeImmutable \$createdAt;

    #[ORM\\Column(nullable: true)]
    private ?\\DateTimeImmutable \$updatedAt = null;

    public function __construct(string \$id)
    {
        \$this->id = \$id;
        \$this->createdAt = new \\DateTimeImmutable();
    }

    #[ORM\\PreUpdate]
    public function onPreUpdate(): void
    {
        \$this->updatedAt = new \\DateTimeImmutable();
    }

    public function getId(): string { return \$this->id; }
    public function getCreatedAt(): \\DateTimeImmutable { return \$this->createdAt; }
    public function getUpdatedAt(): ?\\DateTimeImmutable { return \$this->updatedAt; }
}"

# ─── 2. Repository Interface ──────────────────────────────────────────────────
write_file "src/Domain/Repository/${RESOURCE}RepositoryInterface.php" "<?php

declare(strict_types=1);

namespace ${BASE_NS}\\Domain\\Repository;

use ${BASE_NS}\\Domain\\Model\\${RESOURCE};

interface ${RESOURCE}RepositoryInterface
{
    public function findById(string \$id): ?${RESOURCE};

    /** @return ${RESOURCE}[] */
    public function findAll(): array;

    public function save(${RESOURCE} \$${RESOURCE_LOWER}): void;

    public function delete(${RESOURCE} \$${RESOURCE_LOWER}): void;
}"

# ─── 3. Doctrine Repository Implementation ───────────────────────────────────
write_file "src/Infrastructure/Persistence/Doctrine${RESOURCE}Repository.php" "<?php

declare(strict_types=1);

namespace ${BASE_NS}\\Infrastructure\\Persistence;

use ${BASE_NS}\\Domain\\Model\\${RESOURCE};
use ${BASE_NS}\\Domain\\Repository\\${RESOURCE}RepositoryInterface;
use Doctrine\\Bundle\\DoctrineBundle\\Repository\\ServiceEntityRepository;
use Doctrine\\Persistence\\ManagerRegistry;

class Doctrine${RESOURCE}Repository extends ServiceEntityRepository
    implements ${RESOURCE}RepositoryInterface
{
    public function __construct(ManagerRegistry \$registry)
    {
        parent::__construct(\$registry, ${RESOURCE}::class);
    }

    public function findById(string \$id): ?${RESOURCE}
    {
        return \$this->find(\$id);
    }

    public function findAll(): array
    {
        return parent::findAll();
    }

    public function save(${RESOURCE} \$${RESOURCE_LOWER}): void
    {
        \$this->getEntityManager()->persist(\$${RESOURCE_LOWER});
        // Flush is handled at the use-case boundary — do not flush here.
    }

    public function delete(${RESOURCE} \$${RESOURCE_LOWER}): void
    {
        \$this->getEntityManager()->remove(\$${RESOURCE_LOWER});
    }
}"

# ─── 4. API Platform Resource ─────────────────────────────────────────────────
write_file "src/ApiPlatform/Resource/${RESOURCE}Resource.php" "<?php

declare(strict_types=1);

namespace ${BASE_NS}\\ApiPlatform\\Resource;

use ApiPlatform\\Metadata\\ApiResource;
use ApiPlatform\\Metadata\\Get;
use ApiPlatform\\Metadata\\GetCollection;
use ApiPlatform\\Metadata\\Post;
use ${BASE_NS}\\ApiPlatform\\Processor\\Create${RESOURCE}Processor;
use ${BASE_NS}\\ApiPlatform\\Provider\\${RESOURCE}Provider;
use ${BASE_NS}\\ApiPlatform\\Provider\\${RESOURCE}CollectionProvider;
use Symfony\\Component\\Serializer\\Attribute\\Groups;
use Symfony\\Component\\Validator\\Constraints as Assert;

#[ApiResource(
    shortName: '${RESOURCE}',
    operations: [
        new GetCollection(
            provider: ${RESOURCE}CollectionProvider::class,
            normalizationContext: ['groups' => ['${RESOURCE_LOWER}:list']],
        ),
        new Get(
            provider: ${RESOURCE}Provider::class,
            normalizationContext: ['groups' => ['${RESOURCE_LOWER}:read']],
        ),
        new Post(
            processor: Create${RESOURCE}Processor::class,
            denormalizationContext: ['groups' => ['${RESOURCE_LOWER}:create']],
            normalizationContext: ['groups' => ['${RESOURCE_LOWER}:read']],
            validationContext: ['groups' => ['${RESOURCE_LOWER}:create']],
        ),
    ],
)]
class ${RESOURCE}Resource
{
    #[Groups(['${RESOURCE_LOWER}:read', '${RESOURCE_LOWER}:list'])]
    public ?string \$id = null;

    // TODO: add resource fields with serialization groups

    #[Groups(['${RESOURCE_LOWER}:read'])]
    public ?\\DateTimeImmutable \$createdAt = null;
}"

# ─── 5. State Processor ───────────────────────────────────────────────────────
write_file "src/ApiPlatform/Processor/Create${RESOURCE}Processor.php" "<?php

declare(strict_types=1);

namespace ${BASE_NS}\\ApiPlatform\\Processor;

use ApiPlatform\\Metadata\\Operation;
use ApiPlatform\\State\\ProcessorInterface;
use ${BASE_NS}\\ApiPlatform\\Resource\\${RESOURCE}Resource;

final class Create${RESOURCE}Processor implements ProcessorInterface
{
    // TODO: inject use case via constructor

    public function process(mixed \$data, Operation \$operation,
        array \$uriVariables = [], array \$context = []): ${RESOURCE}Resource
    {
        /** @var ${RESOURCE}Resource \$data */

        // 1. Extract idempotency key
        \$idempotencyKey = \$context['request']->headers->get('Idempotency-Key')
            ?? throw new \\InvalidArgumentException('Idempotency-Key header is required.');

        // 2. TODO: delegate to use case

        // 3. Map domain → resource
        \$resource = new ${RESOURCE}Resource();
        // TODO: populate resource fields
        return \$resource;
    }
}"

# ─── 6. State Provider ────────────────────────────────────────────────────────
write_file "src/ApiPlatform/Provider/${RESOURCE}Provider.php" "<?php

declare(strict_types=1);

namespace ${BASE_NS}\\ApiPlatform\\Provider;

use ApiPlatform\\Metadata\\Operation;
use ApiPlatform\\State\\ProviderInterface;
use ${BASE_NS}\\Domain\\Repository\\${RESOURCE}RepositoryInterface;
use ${BASE_NS}\\ApiPlatform\\Resource\\${RESOURCE}Resource;
use Symfony\\Component\\HttpKernel\\Exception\\NotFoundHttpException;

final class ${RESOURCE}Provider implements ProviderInterface
{
    public function __construct(
        private readonly ${RESOURCE}RepositoryInterface \$repository,
    ) {}

    public function provide(Operation \$operation, array \$uriVariables = [],
        array \$context = []): ${RESOURCE}Resource
    {
        \$entity = \$this->repository->findById(\$uriVariables['id'])
            ?? throw new NotFoundHttpException('${RESOURCE} not found.');

        \$resource = new ${RESOURCE}Resource();
        \$resource->id = \$entity->getId();
        \$resource->createdAt = \$entity->getCreatedAt();
        // TODO: map remaining fields
        return \$resource;
    }
}"

# ─── 7. Domain Unit Test ──────────────────────────────────────────────────────
write_file "tests/Unit/Domain/${RESOURCE}Test.php" "<?php

declare(strict_types=1);

namespace ${BASE_NS}\\Tests\\Unit\\Domain;

use ${BASE_NS}\\Domain\\Model\\${RESOURCE};
use PHPUnit\\Framework\\TestCase;

class ${RESOURCE}Test extends TestCase
{
    public function testCreate${RESOURCE}(): void
    {
        \$entity = new ${RESOURCE}('test-uuid-1');
        \$this->assertSame('test-uuid-1', \$entity->getId());
        \$this->assertInstanceOf(\\DateTimeImmutable::class, \$entity->getCreatedAt());
    }

    // TODO: add domain behaviour tests
}"

# ─── 8. API Integration Test ──────────────────────────────────────────────────
write_file "tests/Api/${RESOURCE}Test.php" "<?php

declare(strict_types=1);

namespace ${BASE_NS}\\Tests\\Api;

use ApiPlatform\\Symfony\\Bundle\\Test\\ApiTestCase;

class ${RESOURCE}Test extends ApiTestCase
{
    public function testCreate${RESOURCE}ReturnsCreated(): void
    {
        \$client = static::createClient();

        // TODO: authenticate client (get JWT token)
        // \$token = \$this->getJwtToken(\$client, \$user);

        \$client->request('POST', '/api/${RESOURCE_PLURAL}', [
            'json' => [
                // TODO: add required fields
            ],
            'headers' => [
                // 'Authorization' => \"Bearer {\$token}\",
                'Idempotency-Key' => 'test-${RESOURCE_LOWER}-create-1',
            ],
        ]);

        \$this->assertResponseStatusCodeSame(201);
        // TODO: assertJsonContains(['field' => 'expected_value']);
    }

    public function testGet${RESOURCE}ReturnsNotFoundForUnknownId(): void
    {
        \$client = static::createClient();
        \$client->request('GET', '/api/${RESOURCE_PLURAL}/nonexistent-id');
        \$this->assertResponseStatusCodeSame(404);
    }
}"

echo ""
echo "✅ Scaffold complete for: $RESOURCE"
echo ""
echo "📋 Next steps:"
echo "  1. Add domain fields to src/Domain/Model/${RESOURCE}.php"
echo "  2. Wire the use case in src/ApiPlatform/Processor/Create${RESOURCE}Processor.php"
echo "  3. Register the Doctrine repository in config/services.yaml:"
echo "     ${BASE_NS}\\Domain\\Repository\\${RESOURCE}RepositoryInterface:"
echo "       alias: ${BASE_NS}\\Infrastructure\\Persistence\\Doctrine${RESOURCE}Repository"
echo "  4. Generate migration: php bin/console doctrine:migrations:diff"
echo "  5. Run tests: php bin/phpunit tests/Unit/Domain/${RESOURCE}Test.php"
