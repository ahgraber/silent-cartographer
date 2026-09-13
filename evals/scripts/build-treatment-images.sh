#!/bin/sh
# Build every treatment task image, timing each build and measuring the baked
# index, then record one build per line for merging into the index build
# manifest, which records each task's one-time index build cost separately
# from the per-episode treatment measures.
#
# Usage: build-treatment-images.sh <treatment-tree-dir> <records.jsonl> [platform]
#   platform: linux/amd64 (default) or linux/arm64 — must match the injected c10r binary's arch.
# Requires a running Docker-compatible daemon and network (repo clone + indexer install).
#
# Each record is appended as it is measured, and the file is never truncated, so a build
# killed partway keeps what it finished and the next run merges it. Merging is keyed by
# instance and the last record for an instance wins, so re-measuring is safe.
# The caller merges the records; this script never rewrites the manifest.
set -eu

TREE_DIR=${1:?usage: build-treatment-images.sh <treatment-tree-dir> <records.jsonl> [platform]}
RECORDS=${2:?usage: build-treatment-images.sh <treatment-tree-dir> <records.jsonl> [platform]}
PLATFORM=${3:-linux/amd64}

task_dirs=""
for task_dir in "${TREE_DIR}"/*/; do
  [ -f "${task_dir}/task.toml" ] && task_dirs="${task_dirs} ${task_dir}"
done
if [ -z "${task_dirs}" ]; then
  echo "no task directories under ${TREE_DIR} — generate and inject the tasks first" >&2
  exit 1
fi

recorded=0
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

  printf '{"instance_id":"%s","wall_clock_s":%s,"index_bytes":%s}\n' \
    "${instance}" "${wall}" "${index_bytes}" >> "${RECORDS}"
  recorded=$((recorded + 1))
  echo "    ${wall}s, index ${index_bytes} bytes"
done

echo "==> appended ${recorded} build record(s) to ${RECORDS}"
if [ -n "${failed}" ]; then
  echo "==> FAILED images (fix and rerun; successes are cached):${failed}"
  exit 1
fi
