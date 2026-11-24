#!/bin/sh
here="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
sys="$(uname -s)"
case "$sys" in
MINGW* | MSYS* | CYGWIN*) rel="x86_64-pc-windows-msvc/stats.exe" ;;
Darwin*)
    case "$(uname -m)" in
    arm64) rel="aarch64-apple-darwin/stats" ;;
    *) rel="x86_64-apple-darwin/stats" ;;
    esac ;;
*)
    case "$(uname -m)" in
    aarch64 | arm64) rel="aarch64-unknown-linux-musl/stats" ;;
    *) rel="x86_64-unknown-linux-musl/stats" ;;
    esac ;;
esac
if [ -x "$here/$rel" ]; then exec "$here/$rel" "$@"; fi
for cand in "$here/target/release/stats.exe" "$here/target/release/stats" "$here/target/debug/stats.exe" "$here/target/debug/stats"; do
    if [ -x "$cand" ]; then exec "$cand" "$@"; fi
done
echo "stats binary not found in $here" >&2
exit 1
