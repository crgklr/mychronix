#!/bin/bash
# Points the Homebrew tap (github.com/<owner>/homebrew-mychronix) at the newest
# release's Mac download, so `brew upgrade` brings it. Pushes with the tap's
# deploy key, in HOMEBREW_TAP_KEY.
#
# Needs: gh (GH_TOKEN, GH_REPO), git, ssh. The release workflow runs this
# (publish.yml).
set -euo pipefail

repo=${GH_REPO:?set GH_REPO, e.g. crgklr/mychronix}
owner=${repo%%/*}
tap="$owner/homebrew-mychronix"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

tag=$(gh release list --exclude-drafts --exclude-pre-releases --limit 1 --json tagName -q '.[0].tagName')
version=${tag#v}
gh release download "$tag" --pattern SHA256SUMS-macos --dir "$work"
sha=$(awk -v f="Mychronix-$version-arm64.dmg" '$2 == f || $2 == "*" f {print $1}' "$work/SHA256SUMS-macos")
[ -n "$sha" ] || { echo "no checksum for Mychronix-$version-arm64.dmg in $tag"; exit 1; }

key="$work/key"
printf '%s\n' "${HOMEBREW_TAP_KEY:?set HOMEBREW_TAP_KEY}" > "$key"
chmod 600 "$key"
export GIT_SSH_COMMAND="ssh -i $key -o IdentitiesOnly=yes -o StrictHostKeyChecking=accept-new"
git clone --quiet --depth 1 "git@github.com:$tap.git" "$work/tap"

mkdir -p "$work/tap/Casks"
cat > "$work/tap/Casks/mychronix.rb" <<EOF
# Updated by Mychronix's release workflow (scripts/publish-homebrew.sh in
# github.com/$repo); changes made here are replaced at the next release.
cask "mychronix" do
  version "$version"
  sha256 "$sha"

  url "https://github.com/$repo/releases/download/v#{version}/Mychronix-#{version}-arm64.dmg"
  name "Mychronix"
  desc "Download and analyze AiM MyChron kart data logger sessions"
  homepage "https://github.com/$repo"

  livecheck do
    url :url
    strategy :github_latest
  end

  depends_on arch: :arm64

  app "Mychronix.app"
  binary "#{appdir}/Mychronix.app/Contents/MacOS/mychronix-cli"

  caveats <<~CAVEATS
    Mychronix isn't signed by Apple yet. The first time you open it (and after
    each update), macOS says it couldn't verify it: click Done, open System
    Settings → Privacy & Security, and click Open Anyway next to Mychronix.
  CAVEATS
end
EOF

cd "$work/tap"
git add -A
if git diff --cached --quiet; then
  echo "the tap already has Mychronix $version"
  exit 0
fi
git -c user.name="Mychronix release" -c user.email="github-actions[bot]@users.noreply.github.com" commit --quiet -m "Mychronix $version"
git push --quiet origin HEAD
echo "the tap now has Mychronix $version"
