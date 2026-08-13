#!/usr/bin/env bash
# End-to-end smoke test for oci-sync against a running OCI registry.
#
# Environment:
#   OCI_SYNC_TEST_REPO   required, e.g. localhost:5000/oci-sync-e2e/ci
#   OCI_SYNC_BIN         binary path (default: target/release/oci-sync)
#   OCI_SYNC_PASSPHRASE  encryption passphrase (default: ci-passphrase)
#   OCI_SYNC_TAG_BASE    tag prefix (default: ci)
#
# The registry must be reachable before running. With Docker:
#   docker run -d -p 5000:5000 registry:2
# Or use the local CNCF distribution setup (see /root/oci-registry).
set -euo pipefail

BIN=${OCI_SYNC_BIN:-target/release/oci-sync}
REPO=${OCI_SYNC_TEST_REPO:?OCI_SYNC_TEST_REPO is required}
PASS=${OCI_SYNC_PASSPHRASE:-ci-passphrase}
BASE=${OCI_SYNC_TAG_BASE:-ci}

WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

pass=0
fail=0
ok()   { echo "  ✓ $1"; pass=$((pass + 1)); }
bad()  { echo "  ✗ $1"; fail=$((fail + 1)); }

expect_fail() { # expect_fail <desc> <cmd...>
  local desc=$1; shift
  if "$@" >/dev/null 2>&1; then
    bad "$desc (expected failure but succeeded)"
  else
    ok "$desc"
  fi
}

echo "== e2e: $BIN -> $REPO =="

# --- setup -------------------------------------------------------------
mkdir -p "$WORK/mydir/sub"
echo "hello oci-sync" > "$WORK/mydir/readme.txt"
echo "nested data"   > "$WORK/mydir/sub/config.ini"
echo "top secret"    > "$WORK/secret.txt"

# --- push ---------------------------------------------------------------
echo "-- push"
"$BIN" push -l "$WORK/mydir" -r "$REPO:${BASE}-plain" --label app=web --label env=prod >/dev/null 2>&1 \
  && ok "push plaintext dir" || bad "push plaintext dir"
"$BIN" push -l "$WORK/secret.txt" -r "$REPO:${BASE}-enc" --passphrase "$PASS" >/dev/null 2>&1 \
  && ok "push encrypted file" || bad "push encrypted file"

# --- list ---------------------------------------------------------------
echo "-- list"
JSON=$("$BIN" list -r "$REPO" -f json)
echo "$JSON" | grep -q "${BASE}-plain" && ok "list shows plain tag" || bad "list shows plain tag"
echo "$JSON" | grep -q "${BASE}-enc"   && ok "list shows enc tag"   || bad "list shows enc tag"
echo "$JSON" | python3 -c "
import json,sys
arts = json.load(sys.stdin)
enc = [a for a in arts if a['tag'] == '${BASE}-enc'][0]
plain = [a for a in arts if a['tag'] == '${BASE}-plain'][0]
assert enc['encrypted'] is True, 'enc tag must be marked encrypted'
assert plain['encrypted'] is False, 'plain tag must not be encrypted'
assert plain['labels'].get('app') == 'web', 'labels must be preserved'" \
  && ok "manifest metadata correct" || bad "manifest metadata correct"

# --- pull (plaintext) ---------------------------------------------------
echo "-- pull plaintext"
"$BIN" pull -r "$REPO:${BASE}-plain" -l "$WORK/out-plain" >/dev/null 2>&1 \
  && diff -r "$WORK/mydir" "$WORK/out-plain/mydir" >/dev/null 2>&1 \
  && ok "pull plaintext matches" || bad "pull plaintext matches"

# --- pull (encrypted) ---------------------------------------------------
echo "-- pull encrypted"
expect_fail "pull encrypted without passphrase fails fast" \
  "$BIN" pull -r "$REPO:${BASE}-enc" -l "$WORK/out-enc"
"$BIN" pull -r "$REPO:${BASE}-enc" -l "$WORK/out-enc" --passphrase "$PASS" >/dev/null 2>&1 \
  && diff "$WORK/secret.txt" "$WORK/out-enc/secret.txt" >/dev/null 2>&1 \
  && ok "pull encrypted with passphrase matches" || bad "pull encrypted with passphrase matches"
expect_fail "pull encrypted with wrong passphrase fails" \
  "$BIN" pull -r "$REPO:${BASE}-enc" -l "$WORK/out-enc2" --passphrase wrong-pass

# --- labels -------------------------------------------------------------
echo "-- labels"
"$BIN" label set -r "$REPO:${BASE}-enc" team=security >/dev/null 2>&1 \
  && ok "label set" || bad "label set"
"$BIN" list -r "$REPO" --label team=security -f json | grep -q "${BASE}-enc" \
  && ok "label filter finds artifact" || bad "label filter finds artifact"
"$BIN" label unset -r "$REPO:${BASE}-enc" team >/dev/null 2>&1 \
  && ok "label unset" || bad "label unset"

# --- shortcuts ----------------------------------------------------------
echo "-- shortcuts"
mkdir -p "$WORK/cfg"
XDG_CONFIG_HOME="$WORK/cfg" "$BIN" alias add e2e --repo "$REPO" >/dev/null 2>&1 \
  && ok "alias add" || bad "alias add"
XDG_CONFIG_HOME="$WORK/cfg" "$BIN" e2e push -l "$WORK/mydir" -t "${BASE}-shortcut" >/dev/null 2>&1 \
  && ok "shortcut push" || bad "shortcut push"
XDG_CONFIG_HOME="$WORK/cfg" "$BIN" e2e list -f json | grep -q "${BASE}-shortcut" \
  && ok "shortcut list" || bad "shortcut list"
XDG_CONFIG_HOME="$WORK/cfg" "$BIN" e2e pull -t "${BASE}-shortcut" -l "$WORK/out-shortcut" >/dev/null 2>&1 \
  && diff -r "$WORK/mydir" "$WORK/out-shortcut/mydir" >/dev/null 2>&1 \
  && ok "shortcut pull matches" || bad "shortcut pull matches"

# --- delete -------------------------------------------------------------
echo "-- delete"
"$BIN" delete -r "$REPO:${BASE}-plain" --yes >/dev/null 2>&1 \
  && ok "delete plain" || bad "delete plain"
XDG_CONFIG_HOME="$WORK/cfg" "$BIN" e2e delete -t "${BASE}-shortcut" --yes >/dev/null 2>&1 \
  && ok "shortcut delete" || bad "shortcut delete"
expect_fail "delete without --yes on non-TTY is rejected" \
  "$BIN" delete -r "$REPO:${BASE}-enc"

# --- catalog ------------------------------------------------------------
echo "-- catalog"
REG=${REPO%%/*}
"$BIN" list -r "$REG" -f json | grep -q "${BASE}-enc" \
  && ok "registry catalog lists artifact" || bad "registry catalog lists artifact"

# --- cleanup ------------------------------------------------------------
"$BIN" delete -r "$REPO:${BASE}-enc" --yes >/dev/null 2>&1 || true

echo
echo "== result: $pass passed, $fail failed =="
[ "$fail" -eq 0 ]
