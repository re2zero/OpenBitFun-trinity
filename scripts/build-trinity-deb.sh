#!/usr/bin/env bash
# One-shot build: Trinity cognitive daemon (trinityd) + OpenBitFun deb.
#
# Steps:
#   1. cargo build --release -p trinityd in the trinity repo
#   2. stage it as the tauri externalBin sidecar (trinityd-<target triple>)
#      so the deb installs it to /usr/bin/ next to the main binary —
#      trinity::backend::locate_trinityd finds it via the exe-dir branch
#   3. reuse the standard desktop deb entry point
#
# Env overrides:
#   TRINITY_DIR   trinity repo checkout (default: ../trinity next to BitFun)
#   SKIP_TRINITYD_BUILD=1  reuse an already-built trinityd binary
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
if [[ -n "${TRINITY_DIR:-}" ]]; then
  TRINITY_DIR="$TRINITY_DIR"
else
  # Probe common checkouts next to / near the BitFun repo.
  for candidate in "$ROOT/../trinity" "$ROOT/../sublime/trinity"; do
    if [[ -f "$candidate/Cargo.toml" && -d "$candidate/trinityd" ]]; then
      TRINITY_DIR="$(cd "$candidate" && pwd)"
      break
    fi
  done
fi
[[ -d "${TRINITY_DIR:-}" ]] || {
  echo "error: trinity repo not found; set TRINITY_DIR=/path/to/trinity" >&2
  exit 1
}
TRIPLE="$(rustc -vV | awk '/^host:/{print $2}')"
SIDECAR="$ROOT/src/apps/desktop/trinityd-$TRIPLE"

echo "==> [1/3] building trinityd (release) in $TRINITY_DIR"
if [[ "${SKIP_TRINITYD_BUILD:-0}" != "1" ]]; then
  (cd "$TRINITY_DIR" && cargo build --release -p trinityd)
fi
TRINITYD="$TRINITY_DIR/target/release/trinityd"
[[ -x "$TRINITYD" ]] || { echo "error: $TRINITYD not found" >&2; exit 1; }

echo "==> [2/3] staging sidecar $SIDECAR"
cp -f "$TRINITYD" "$SIDECAR"
chmod +x "$SIDECAR"

echo "==> [3/3] building desktop deb"
export PATH="$HOME/.local/node22/bin:$HOME/.local/bin:$PATH"
export OPENBITFUN_TARGET_GC=0
cd "$ROOT"
pnpm run desktop:build:linux:deb

echo
echo "==> done:"
ls -lh "$ROOT"/target/release/bundle/deb/*.deb
echo "install check: dpkg -c "$ROOT"/target/release/bundle/deb/*.deb | grep trinityd"
