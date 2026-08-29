#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
cargo_bin="${CARGO:-cargo}"
bench_root="$(mktemp -d "${TMPDIR:-/tmp}/lenso-dx.XXXXXX")"
project_root="$bench_root/greeting"

cleanup() {
  rm -rf "$bench_root"
}
trap cleanup EXIT

now_ms() {
  python3 -c 'import time; print(time.monotonic_ns() // 1_000_000)'
}

measure() {
  local name="$1"
  shift
  local started finished
  started="$(now_ms)"
  "$@" >/dev/null
  finished="$(now_ms)"
  printf '%s\t%s\n' "$name" "$((finished - started))"
}

cd "$repo_root"
"$cargo_bin" build --locked --quiet
target_dir="$("$cargo_bin" metadata --no-deps --format-version 1 | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')"
lenso_bin="$target_dir/debug/lenso"

printf 'metric\tduration_ms\n'
measure scaffold "$lenso_bin" plugin new greeting --repo-root "$bench_root" --runtime wasm --no-install
(
  cd "$project_root"
  "$cargo_bin" generate-lockfile --quiet
)
measure fresh_check env CARGO="$cargo_bin" "$lenso_bin" plugin check --repo-root "$project_root"
touch "$project_root/src/lib.rs"
measure incremental_check env CARGO="$cargo_bin" "$lenso_bin" plugin check --repo-root "$project_root"
measure dev_invoke env CARGO="$cargo_bin" "$lenso_bin" plugin dev --repo-root "$project_root" --operation execute --request-json '{"name":"greeting","arguments_json":"{\"text\":\"hello\"}"}'
measure release_pack env CARGO="$cargo_bin" "$lenso_bin" plugin pack --repo-root "$project_root"
