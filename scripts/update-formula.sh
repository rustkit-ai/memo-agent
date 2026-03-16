#!/usr/bin/env bash
# Updates Formula/memo.rb with correct SHA256 hashes from a GitHub release.
# Usage: ./scripts/update-formula.sh <version>
# Example: ./scripts/update-formula.sh 0.1.1
set -euo pipefail

VERSION="${1:?usage: $0 <version>}"
REPO="rustkit-ai/memo-agent"
FORMULA="Formula/memo.rb"

targets=(
  "aarch64-apple-darwin"
  "x86_64-apple-darwin"
  "aarch64-unknown-linux-gnu"
  "x86_64-unknown-linux-gnu"
)

echo "Fetching SHA256 for v$VERSION..."

for target in "${targets[@]}"; do
  asset="memo-${target}.tar.gz"
  url="https://github.com/${REPO}/releases/download/v${VERSION}/${asset}"
  echo -n "  $target ... "
  sha=$(curl -fsSL "$url" | sha256sum | awk '{print $1}')
  echo "$sha"
  # Replace :no_check with the real SHA for this target
  # Match the line after the url line for this target
  sed -i.bak "/memo-${target}\.tar\.gz/{n;s/sha256 .*/sha256 \"${sha}\"/}" "$FORMULA"
done

# Update version
sed -i.bak "s/version \".*\"/version \"${VERSION}\"/" "$FORMULA"
rm -f "${FORMULA}.bak"

echo "Updated $FORMULA"
