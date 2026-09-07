#!/usr/bin/env bash
set -euo pipefail

log_file=$(mktemp)
trap 'rm -f "$log_file"' EXIT

set +e
nix build . 2>&1 | tee "$log_file"
status=${PIPESTATUS[0]}
set -e

if [ "$status" -ne 0 ]; then
  hash=$(grep -oE 'got:[[:space:]]*sha256-[A-Za-z0-9+/=]+' "$log_file" | sed -E 's/^got:[[:space:]]*//' | tail -n1 || true)
  rustc_version=$(grep -oE 'rustc [0-9]+\.[0-9]+\.[0-9]+ is not supported' "$log_file" | head -n1 || true)
  required_rust=$(grep -oE 'rust-version[[:space:]]*=[[:space:]]*"[0-9]+\.[0-9]+"' "$log_file" | head -n1 || true)

  if [ -n "$hash" ]; then
    echo "Expected cargoHash: $hash"

    if [ -n "${GITHUB_ACTIONS:-}" ]; then
      echo "::error::Nix cargo hash mismatch. Update flake.nix to use $hash"
    fi

    if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
      {
        echo "### Nix cargo hash mismatch"
        echo
        echo "Set cargoHash in flake.nix to:"
        echo
        echo '```nix'
        echo "$hash"
        echo '```'
      } >> "$GITHUB_STEP_SUMMARY"
    fi
  elif [ -n "$rustc_version" ]; then
    message="Nix build failed: $rustc_version. The flake's rustc is older than the workspace's required Rust version."
    if [ -n "$required_rust" ]; then
      message="Nix build failed: $rustc_version (required $required_rust). Update flake.nix to use a newer Rust toolchain."
    fi
    echo "$message"

    if [ -n "${GITHUB_ACTIONS:-}" ]; then
      echo "::error::$message"
    fi

    if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
      {
        echo "### Nix rustc version mismatch"
        echo
        echo "$message"
        echo
        echo "Update the Rust toolchain in flake.nix to be >= the workspace rust-version."
      } >> "$GITHUB_STEP_SUMMARY"
    fi
  else
    message="Nix build failed. Could not extract a replacement cargo hash or rustc version mismatch from the build log."
    echo "$message"

    if [ -n "${GITHUB_ACTIONS:-}" ]; then
      echo "::error::$message"
    fi
  fi

  exit "$status"
fi

nix run . -- --help
