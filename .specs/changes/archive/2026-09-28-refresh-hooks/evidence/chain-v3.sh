#!/bin/sh
# Chaining check against the built c10r binary and the installed prek.
# Steps: H (user hook at E), L (stray foreign E.legacy), prek, prek-f, prek-un, c10r (hooks install),
# c10r-un (hooks uninstall). After each step, a commit fires post-commit and the line reports which
# hooks ran, how many c10r refreshes ran, and the files left in the hooks directory.
# Requires the c10r under test first on PATH, prek on PATH, and rust-analyzer on PATH.
set -u
SEQ="$1"
d="${TMPDIR:-/tmp}/chain-v3"
rm -rf "${d}"
mkdir -p "${d}/src"
cd "${d}" || exit 1
git init -q -b main
git config user.email t@t
git config user.name t
git config commit.gpgsign false
log="${d}.log"
hooks=.git/hooks
ev=post-commit
cat > .pre-commit-config.yaml << EOF
default_install_hook_types: [post-commit]
repos:
  - repo: local
    hooks:
      - id: log
        name: log
        entry: sh -c 'echo prek >> ${log}'
        language: system
        stages: [post-commit]
        always_run: true
        pass_filenames: false
EOF
printf '[package]\nname = "chain"\nversion = "0.1.0"\nedition = "2021"\n' > Cargo.toml
printf 'pub fn chain() -> u32 {\n    1\n}\n' > src/lib.rs
printf '.c10r/\ntarget/\n' > .gitignore
git add .pre-commit-config.yaml Cargo.toml src/lib.rs .gitignore
git commit -qm init

n=0
for step in ${SEQ}; do
  case "${step}" in
    H)
      printf '#!/bin/sh\necho H >> %s\n' "${log}" > "${hooks}/${ev}"
      chmod +x "${hooks}/${ev}"
      ;;
    L)
      printf '#!/bin/sh\necho L >> %s\n' "${log}" > "${hooks}/${ev}.legacy"
      chmod +x "${hooks}/${ev}.legacy"
      ;;
    prek) prek install -q > /dev/null 2>&1 ;;
    prek-f) prek install -f -q > /dev/null 2>&1 ;;
    prek-un) prek uninstall -q > /dev/null 2>&1 ;;
    c10r)
      if ! c10r hooks install > /dev/null 2> "${d}.install-err"; then
        printf '  c10r: REFUSED: '
        cat "${d}.install-err"
      fi
      ;;
    c10r-un) c10r hooks uninstall > /dev/null 2>&1 ;;
    *) echo "unknown step ${step}" ;;
  esac
  n=$((n + 1))
  : > "${log}"
  echo "${n}" > "f${n}"
  git add "f${n}"
  out=$(git commit -qm "${n}" 2>&1)
  ran=$(tr '\n' ' ' < "${log}")
  refreshes=$(echo "${out}" | grep -c -e '^built' -e '^already current')
  files=""
  for path in "${hooks}"/*; do
    case "${path}" in
      *.sample | "${hooks}/*") ;;
      *) files="${files}${path##*/} " ;;
    esac
  done
  files="${files% }"
  err=""
  echo "${out}" | grep -q -e 'migration mode' -e 'failed' && err=" [error output: $(echo "${out}" | grep -e 'migration mode' -e 'failed' | head -1)]"
  printf '%-8s runs: %-12s c10r refreshes: %s  files: %s%s\n' "${step}" "${ran:-(none)}" "${refreshes}" "${files}" "${err}"
done
