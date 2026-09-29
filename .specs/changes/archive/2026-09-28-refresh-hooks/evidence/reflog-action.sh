#!/bin/sh
# Scratch check: what GIT_REFLOG_ACTION and the rebase directories look like to each post-* hook,
# for both rebase backends, with and without commits to replay, and for plain checkouts.
set -eu
d="${TMPDIR:-/tmp}/reflog-action"
rm -rf "${d}"
mkdir -p "${d}"
cd "${d}"
git init -q -b main
git config user.email t@t
git config user.name t
git config commit.gpgsign false
log="${d}.log"
for ev in post-checkout post-commit post-rewrite post-merge post-applypatch; do
  cat > ".git/hooks/${ev}" << EOF
#!/bin/sh
s=idle
[ -d "\$(git rev-parse --git-path rebase-merge)" ] && s=merge-dir
[ -d "\$(git rev-parse --git-path rebase-apply)" ] && s=apply-dir
echo "${ev} [\$s] action='\${GIT_REFLOG_ACTION:-}' args=\$(echo "\$*" | sed 's/[0-9a-f]\{40\}/<sha>/g')" >> ${log}
EOF
  chmod +x ".git/hooks/${ev}"
done
echo a > a
git add a
git commit -qm a
git switch -qc topic
echo 1 > one
git add one
git commit -qm one
echo 2 > two
git add two
git commit -qm two
git switch -q main
echo u > up
git add up
git commit -qm up
git switch -q topic

run() {
  label="$1"
  shift
  : > "${log}"
  "$@" > /dev/null 2>&1 || echo "  (${label} exited nonzero)"
  echo "--- ${label}"
  cat "${log}"
}

run "merge backend, 2 commits to replay" git rebase -q main
git reset -q --hard ORIG_HEAD
run "apply backend, 2 commits to replay" git rebase -q --apply --no-update-refs main
git switch -q main
git switch -qc behind HEAD~1
run "merge backend, nothing to replay" git rebase -q main
git reset -q --hard main~1
run "apply backend, nothing to replay" git rebase -q --apply --no-update-refs main
run "plain switch" git switch -q topic
run "switch -c at same commit" git switch -qc same
run "switch --detach" git switch -q --detach main
