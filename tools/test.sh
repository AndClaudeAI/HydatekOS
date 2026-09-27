#!/usr/bin/env bash
# Run HydatekOS's automated tests that don't need a running VM:
#   kernel crypto/QR/HLP and Hyda Scripts documents (.docx, zip, inflate) (Rust,
#   on the host), web companion crypto (Node),
#   Android protocol code (JVM). All three check the same interop vectors.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
echo "== kernel modules (cargo test)"
(cd "$ROOT/tests-host" && cargo test --quiet)
echo "== web companion (node)"
node "$ROOT/companion/web/test/hlp.test.js"
echo "== Android protocol code (JVM)"
OUT="$(mktemp -d)"
javac --release 8 -Xlint:-options -nowarn -d "$OUT" "$ROOT"/companion/android/src/org/hydatek/link/{Hlp,LinkClient,Pairing}.java "$ROOT/companion/android/test/HlpTest.java"
java -cp "$OUT" HlpTest "$ROOT/companion/test-vectors.json" "$@"
rm -rf "$OUT"
