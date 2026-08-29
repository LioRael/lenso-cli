#!/usr/bin/env bash
set -euo pipefail

usage() {
  printf 'Usage: tools/contributor/install.sh --framework-root <absolute-path>\n'
}

framework_root=""
while [ "$#" -gt 0 ]; do
  case "$1" in
    --framework-root) framework_root="${2:-}"; shift ;;
    -h|--help) usage; exit 0 ;;
    *) printf 'Unknown option: %s\n' "$1" >&2; usage >&2; exit 2 ;;
  esac
  shift
done

case "$framework_root" in
  /*) ;;
  *) printf 'ERROR --framework-root must be an absolute path\n' >&2; exit 2 ;;
esac
if [ ! -d "$framework_root" ]; then
  printf 'ERROR framework root does not exist: %s\n' "$framework_root" >&2
  exit 1
fi

script_directory="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
target_directory="$framework_root/.lenso-tools/bin"
mkdir -p "$target_directory"

for tool_name in lenso-workspace lenso-pr; do
  source_path="$script_directory/bin/$tool_name"
  temporary_path="$(mktemp "$target_directory/.$tool_name.XXXXXX")"
  install -m 0755 "$source_path" "$temporary_path"
  mv "$temporary_path" "$target_directory/$tool_name"
  printf 'Installed %s\n' "$target_directory/$tool_name"
done
