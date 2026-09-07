#!/bin/sh
# Build every treatment task image, timing each build and measuring the baked
# index, then write the build manifest the analysis break-even derivation reads.
#
# Usage: build-treatment-images.sh <treatment-tree-dir> <build-manifest.json> [platform]
#   platform: linux/amd64 (default) or linux/arm64 — must match the injected c10r binary's arch.
# Requires a running Docker-compatible daemon and network (repo clone + indexer install).
set -eu

TREE_DIR=${1:?usage: build-treatment-images.sh <treatment-tree-dir> <build-manifest.json> [platform]}
MANIFEST=${2:?usage: build-treatment-images.sh <treatment-tree-dir> <build-manifest.json> [platform]}
PLATFORM=${3:-linux/amd64}

entries=""
failed=""
for task_dir in "${TREE_DIR}"/*/; do
  [ -f "${task_dir}/task.toml" ] || continue
  instance=$(basename "${task_dir}")
  tag="c10r-eval-treatment-$(printf '%s' "${instance}" | tr '[:upper:]' '[:lower:]' | tr -c 'a-z0-9._-' '-')"

  echo "==> building ${instance}"
  start=$(date +%s)
  if ! docker build --platform "${PLATFORM}" --load -t "${tag}" "${task_dir}/environment" > /dev/null; then
    echo "    FAILED: ${instance} (recorded; continuing)"
    failed="${failed} ${instance}"
    continue
  fi
  end=$(date +%s)
  wall=$((end - start))

  index_bytes=$(docker run --rm --platform "${PLATFORM}" "${tag}" sh -c 'du -sb /repo/.c10r 2>/dev/null | cut -f1 || echo 0')

  [ -n "${entries}" ] && entries="${entries},"
  entries="${entries}{\"instance_id\":\"${instance}\",\"wall_clock_s\":${wall},\"index_bytes\":${index_bytes}}"
  echo "    ${wall}s, index ${index_bytes} bytes"
done

printf '{"entries":[%s]}\n' "${entries}" > "${MANIFEST}"
echo "==> wrote ${MANIFEST}"
if [ -n "${failed}" ]; then
  echo "==> FAILED images (fix and rerun; successes are cached):${failed}"
  exit 1
fi
