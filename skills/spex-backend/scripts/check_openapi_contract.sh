#!/usr/bin/env bash
# check_openapi_contract.sh
# Compares the live OpenAPI spec exported by API Platform against a committed
# reference contract file. Fails with a non-zero exit code if they differ,
# making it suitable as a CI gate.
#
# Usage:
#   ./skills/spex-backend/scripts/check_openapi_contract.sh [options]
#
# Options:
#   --contract FILE    Path to the committed reference contract
#                      (default: docs/api/openapi.yaml)
#   --output FILE      Path to write the exported spec before diffing
#                      (default: /tmp/openapi-live.yaml)
#   --format FORMAT    Export format: yaml or json (default: yaml)
#   --console PATH     Path to Symfony console (default: bin/console)
#   --update           Write the exported spec over the reference contract
#                      (use to regenerate the committed baseline)
#   --help             Show this help message
#
# Examples:
#   # Check in CI (fails if spec has changed):
#   ./skills/spex-backend/scripts/check_openapi_contract.sh
#
#   # Update committed baseline after intentional API changes:
#   ./skills/spex-backend/scripts/check_openapi_contract.sh --update
#
#   # Custom paths:
#   ./skills/spex-backend/scripts/check_openapi_contract.sh \
#     --contract docs/api/v1.yaml \
#     --console bin/console

set -euo pipefail

# ─── Defaults ─────────────────────────────────────────────────────────────────
CONTRACT_FILE="docs/api/openapi.yaml"
OUTPUT_FILE="/tmp/openapi-live.yaml"
FORMAT="yaml"
CONSOLE="bin/console"
UPDATE=false

# ─── Argument parsing ─────────────────────────────────────────────────────────
while [[ $# -gt 0 ]]; do
  case "$1" in
    --contract)   CONTRACT_FILE="$2"; shift 2 ;;
    --output)     OUTPUT_FILE="$2";   shift 2 ;;
    --format)     FORMAT="$2";        shift 2 ;;
    --console)    CONSOLE="$2";       shift 2 ;;
    --update)     UPDATE=true;        shift   ;;
    --help)
      sed -n '2,30p' "$0" | grep '^#' | sed 's/^# \?//'
      exit 0
      ;;
    *) echo "Unknown option: $1"; exit 1 ;;
  esac
done

# ─── Validation ───────────────────────────────────────────────────────────────
if [[ ! -f "$CONSOLE" ]]; then
  echo "❌ Symfony console not found at: $CONSOLE"
  echo "   Run this script from the Symfony project root, or use --console to specify the path."
  exit 1
fi

if [[ "$FORMAT" != "yaml" && "$FORMAT" != "json" ]]; then
  echo "❌ Invalid format: $FORMAT (must be 'yaml' or 'json')"
  exit 1
fi

# ─── Export live spec ─────────────────────────────────────────────────────────
echo "📤 Exporting live OpenAPI spec..."
php "$CONSOLE" api:openapi:export --format="$FORMAT" --output="$OUTPUT_FILE" 2>/dev/null \
  || php "$CONSOLE" api:openapi:export "--$FORMAT" > "$OUTPUT_FILE"

if [[ ! -s "$OUTPUT_FILE" ]]; then
  echo "❌ Exported spec is empty. Check your API Platform configuration."
  exit 1
fi

echo "   Exported to: $OUTPUT_FILE"

# ─── Update mode ──────────────────────────────────────────────────────────────
if [[ "$UPDATE" == true ]]; then
  mkdir -p "$(dirname "$CONTRACT_FILE")"
  cp "$OUTPUT_FILE" "$CONTRACT_FILE"
  echo ""
  echo "✅ Reference contract updated: $CONTRACT_FILE"
  echo "   Review the diff below and commit if the changes are intentional:"
  echo ""
  git diff -- "$CONTRACT_FILE" 2>/dev/null || true
  exit 0
fi

# ─── Compare against reference ────────────────────────────────────────────────
if [[ ! -f "$CONTRACT_FILE" ]]; then
  echo ""
  echo "⚠️  No reference contract found at: $CONTRACT_FILE"
  echo "   Generate it with: $0 --update"
  echo "   Then commit $CONTRACT_FILE to the repository."
  exit 1
fi

echo "🔍 Comparing against reference: $CONTRACT_FILE"
echo ""

if diff --unified=5 "$CONTRACT_FILE" "$OUTPUT_FILE" > /tmp/openapi-diff.txt 2>&1; then
  echo "✅ OpenAPI contract is unchanged — no drift detected."
  exit 0
else
  echo "❌ OpenAPI CONTRACT DRIFT DETECTED"
  echo "───────────────────────────────────────────────────────────────────────"
  echo "The live spec differs from the committed contract at: $CONTRACT_FILE"
  echo ""
  echo "Diff (reference → live):"
  echo "───────────────────────────────────────────────────────────────────────"
  cat /tmp/openapi-diff.txt
  echo "───────────────────────────────────────────────────────────────────────"
  echo ""
  echo "Possible causes:"
  echo "  1. A new endpoint was added without updating the contract"
  echo "  2. An existing endpoint was changed (path, method, schema)"
  echo "  3. A serialization group was added or removed"
  echo "  4. API Platform metadata annotations were changed"
  echo ""
  echo "To update the committed contract after intentional changes:"
  echo "  $0 --update"
  echo "  git add $CONTRACT_FILE"
  echo "  git commit -m 'docs(api): update OpenAPI contract — Refs: TASK-NNN'"
  echo ""
  echo "To review only (no exit code):"
  echo "  $0 || true"
  exit 1
fi
