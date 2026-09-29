#!/bin/sh
# Scratch check: does prek's shim run an <event>.legacy file that appears after `prek install`?
# And what marks a prek shim so c10r can recognize one?
set -u
d="${TMPDIR:-/tmp}/prek-late-legacy"
rm -rf "${d}"
mkdir -p "${d}"
cd "${d}" || exit
git init -q -b main
git config user.email t@t
git config user.name t
git config commit.gpgsign false
log="${d}.log"
cat > .pre-commit-config.yaml << EOF
default_install_hook_types: [post-commit, post-checkout]
repos:
  - repo: local
    hooks:
      - id: log
        name: log
        entry: sh -c 'echo prek >> ${log}'
        language: system
        stages: [post-commit, post-checkout]
        always_run: true
        pass_filenames: false
EOF
git add .pre-commit-config.yaml
git commit -qm init
prek install -q > /dev/null 2>&1
echo '--- prek shim header'
head -3 .git/hooks/post-commit
printf '#!/bin/sh\necho "late-legacy args=$*" >> %s\nexit 3\n' "${log}" > .git/hooks/post-commit.legacy
chmod +x .git/hooks/post-commit.legacy
cp .git/hooks/post-commit.legacy .git/hooks/post-checkout.legacy
: > "${log}"
echo 1 > f1
git add f1
out=$(git commit -qm 1 2>&1)
echo "commit exit $?"
echo '--- post-commit with a .legacy created after prek install'
cat "${log}"
echo "${out}" | sed 's/^/  out: /'
: > "${log}"
git switch -qc other
echo "switch exit $?"
echo '--- post-checkout: does the legacy exit status (3) reach git?'
cat "${log}"
