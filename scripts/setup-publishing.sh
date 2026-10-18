#!/bin/bash
# One-time setup for publishing (see .github/workflows/publish.yml): makes the
# keys the release workflow needs and stores them as GitHub secrets.
#
#   scripts/setup-publishing.sh <folder to keep the key backups in>
#
# - PACKAGE_SIGNING_KEY: a new signing key for the dnf and apt repositories.
#   Its backup goes in the folder: keep it, and out of git. If it's lost,
#   make a new one, but everyone who added the repositories has to fetch the
#   new public key.
# - HOMEBREW_TAP_KEY: a deploy key that may push to <owner>/homebrew-mychronix
#   and nothing else.
#
# Needs gh (logged in as the repository's owner), gpg and ssh-keygen. A secret
# that's already set is left alone.
set -euo pipefail
cd "$(dirname "$0")/.."

backup=${1:?usage: scripts/setup-publishing.sh <folder to keep the key backups in>}
repo=$(gh repo view --json nameWithOwner -q .nameWithOwner)
tap="${repo%%/*}/homebrew-mychronix"
secrets=$(gh secret list -R "$repo" --json name -q '.[].name')
mkdir -p -m 700 "$backup"

if grep -qx PACKAGE_SIGNING_KEY <<<"$secrets"; then
  echo "PACKAGE_SIGNING_KEY is already set."
else
  export GNUPGHOME
  GNUPGHOME=$(mktemp -d)
  gpg --batch --quiet --passphrase '' --quick-gen-key "Mychronix packages" rsa4096 sign 0
  fpr=$(gpg --list-keys --with-colons | awk -F: '/^fpr/ {print $10; exit}')
  (umask 077; gpg --armor --export-secret-keys "$fpr" > "$backup/mychronix-packages.private.asc")
  gpg --armor --export "$fpr" > "$backup/mychronix-packages.public.asc"
  gh secret set PACKAGE_SIGNING_KEY -R "$repo" < "$backup/mychronix-packages.private.asc"
  rm -rf "$GNUPGHOME"
  echo "PACKAGE_SIGNING_KEY set (fingerprint $fpr); backup in $backup."
fi

if grep -qx HOMEBREW_TAP_KEY <<<"$secrets"; then
  echo "HOMEBREW_TAP_KEY is already set."
else
  gh repo view "$tap" >/dev/null || { echo "Make $tap first."; exit 1; }
  key="$backup/homebrew-tap-deploy-key"
  rm -f "$key" "$key.pub"
  ssh-keygen -q -t ed25519 -N '' -C "Mychronix release workflow" -f "$key"
  gh repo deploy-key add "$key.pub" -R "$tap" --allow-write --title "Mychronix release workflow"
  gh secret set HOMEBREW_TAP_KEY -R "$repo" < "$key"
  echo "HOMEBREW_TAP_KEY set; it may push to $tap only."
fi
