#!/bin/sh
# Build a static musl c10r binary for treatment-arm task images, then prove it runs
# offline in a bare container of the matching platform.
#
# Usage: build-c10r-musl.sh [arch]
#   arch: x86_64 (default) or aarch64.
#   Use aarch64 for native local dev iteration on Apple Silicon (no emulation);
#   the frozen run pins one arch for every sweep and records it.
#
# The build container runs the host's NATIVE architecture and cross-compiles, with zig
# as the cross linker (cargo-zigbuild) — no emulated compiler. Only the verification
# step runs a container of the target platform (one short --version call).
#
# sqlite-vec's C amalgamation uses BSD u_int*_t names that glibc leaks transitively but
# musl headers do not; the CFLAGS defines below map them to their stdint equivalents.
#
# Output: evals/vendor/c10r/<arch>/c10r (+ .sha256), ready to COPY into treatment Dockerfiles.
set -eu

ARCH=${1:-x86_64}
case "${ARCH}" in
  x86_64) PLATFORM=linux/amd64 ;;
  aarch64) PLATFORM=linux/arm64 ;;
  *)
    echo "unsupported arch: ${ARCH} (use x86_64 or aarch64)" >&2
    exit 2
    ;;
esac
TARGET="${ARCH}-unknown-linux-musl"

REPO_ROOT=$(git -C "$(dirname "$0")" rev-parse --show-toplevel)
# Pin to a digest once the first successful build confirms the image.
BUILD_IMAGE=ghcr.io/rust-cross/cargo-zigbuild:latest
VERIFY_IMAGE=alpine:3.20
OUT_DIR="${REPO_ROOT}/evals/vendor/c10r/${ARCH}"

echo "==> cross-compiling c10r for ${TARGET} in a native container"
docker run --rm \
  -v "${REPO_ROOT}":/work -w /work \
  -v c10r-musl-cargo-registry:/usr/local/cargo/registry \
  -e CARGO_TARGET_DIR=/work/target/musl \
  -e "CFLAGS_${ARCH}_unknown_linux_musl=-Du_int8_t=uint8_t -Du_int16_t=uint16_t -Du_int64_t=uint64_t" \
  "${BUILD_IMAGE}" sh -ec "
        rustup show >/dev/null 2>&1 || true   # install the repo-pinned toolchain
        rustup target add ${TARGET} >/dev/null
        cargo zigbuild --release --target ${TARGET}
    "

mkdir -p "${OUT_DIR}"
cp "${REPO_ROOT}/target/musl/${TARGET}/release/c10r" "${OUT_DIR}/c10r"
(cd "${OUT_DIR}" && shasum -a 256 c10r > c10r.sha256)

echo "==> verifying the binary runs offline in a bare ${PLATFORM} container"
docker run --rm --platform "${PLATFORM}" --network none \
  -v "${OUT_DIR}":/vendor:ro \
  "${VERIFY_IMAGE}" /vendor/c10r --version

echo "==> ok: ${OUT_DIR}/c10r"
cat "${OUT_DIR}/c10r.sha256"
