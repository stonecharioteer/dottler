#!/usr/bin/env bash
# Install git hooks via uvx pre-commit.
set -euo pipefail

if ! command -v uvx >/dev/null 2>&1; then
  echo "uvx is not installed. Install uv: https://docs.astral.sh/uv/" >&2
  exit 1
fi

cd "$(dirname "$0")"
uvx pre-commit install --install-hooks --hook-type pre-commit --hook-type commit-msg
echo "hooks installed"
