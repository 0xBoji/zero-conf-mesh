#!/usr/bin/env bash

set -euo pipefail

PACKAGE_NAME="coding_agent_mesh_presence"
BIN_NAME="camp"
REPO_URL="https://github.com/0xBoji/coding_agent_mesh_presence"
RAW_SCRIPT_URL="https://raw.githubusercontent.com/0xBoji/coding_agent_mesh_presence/main/scripts/install.sh"

usage() {
  cat <<'EOF'
Install the `camp` CLI from coding_agent_mesh_presence.

Usage:
  install.sh [--git] [--force]

Options:
  --git    Install directly from the GitHub repository instead of crates.io.
  --force  Reinstall even if the binary is already present.
  -h, --help
EOF
}

require_cmd() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "error: required command not found: $1" >&2
    exit 1
  fi
}

SOURCE="crates"
FORCE=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --git)
      SOURCE="git"
      shift
      ;;
    --force)
      FORCE=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "error: unknown argument: $1" >&2
      usage >&2
      exit 1
      ;;
  esac
done

require_cmd cargo

INSTALL_ARGS=(install --locked --bin "$BIN_NAME")
if [[ "$FORCE" -eq 1 ]]; then
  INSTALL_ARGS+=(--force)
fi

if [[ "$SOURCE" == "git" ]]; then
  echo "Installing ${BIN_NAME} from ${REPO_URL}..."
  cargo "${INSTALL_ARGS[@]}" --git "$REPO_URL"
else
  echo "Installing ${BIN_NAME} from crates.io package ${PACKAGE_NAME}..."
  if ! cargo "${INSTALL_ARGS[@]}" "$PACKAGE_NAME"; then
    cat >&2 <<EOF
error: failed to install ${PACKAGE_NAME} from crates.io.

If the renamed crate has not been published yet, try the GitHub fallback:
  bash <(curl -fsSL ${RAW_SCRIPT_URL}) --git
EOF
    exit 1
  fi
fi

echo
echo "Installed ${BIN_NAME}. Try:"
echo "  ${BIN_NAME} --help"
