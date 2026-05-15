#!/usr/bin/env bash
set -euo pipefail

IMAGE="${IMAGE:-tztcloud/livepeer-network-bot:latest}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

echo "Building ${IMAGE}"
docker build -t "${IMAGE}" "${REPO_ROOT}"
