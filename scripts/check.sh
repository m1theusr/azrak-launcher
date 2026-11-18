#!/usr/bin/env bash
# One round of checks while working on the launcher: build, then the tests and the screenshots
# side by side. Run it in the azrak-dev distrobox; see .claude/skills/azrak-fork/SKILL.md.
#
#   scripts/check.sh                  # 1920x1080 screenshots
#   scripts/check.sh 1280x720 2560x1440
#
# It uses the `quick` profile (Cargo.toml): our code builds with light optimisation and
# incrementally, the libraries fully optimised and once, and the tests share them, so an edit
# rebuilds in seconds. Linked with mold when it's there. Releases still build with --release.
set -euo pipefail
cd "$(dirname "$0")/.."
cargo="${CARGO:-$HOME/.cargo/bin/cargo}"
if [ -x /usr/libexec/mold/ld ]; then
    export RUSTFLAGS="${RUSTFLAGS:-} -C link-arg=-B/usr/libexec/mold"
fi
# Screens open on a virtual display, never on the desktop this runs from.
unset WAYLAND_DISPLAY XDG_SESSION_TYPE

start=$(date +%s)
"$cargo" build --profile quick --locked
"$cargo" test --profile quick --locked --no-run 2>/dev/null
echo "built in $(( $(date +%s) - start ))s"

log=$(mktemp -d)
"$cargo" test --profile quick --locked >"$log/tests" 2>&1 & tests=$!
SWARMPLAY_BIN=target/quick/swarmplay python3 scripts/ui_matrix.py "${@:-1920x1080}" >"$log/shots" 2>&1 & shots=$!

status=0
wait $tests || status=1
grep -E "^test result|FAILED|panicked" "$log/tests" || true
wait $shots || { status=1; tail -20 "$log/shots"; }
echo "screenshots: dist/ui-matrix/ · done in $(( $(date +%s) - start ))s"
exit $status
