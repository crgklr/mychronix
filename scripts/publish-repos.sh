#!/bin/bash
# Builds Mychronix's dnf and apt repositories into target/site, for GitHub
# Pages: the .rpm and .deb from the newest few GitHub releases, signed, with
# the repository files and a page saying how to add them. Rebuilt from the
# releases every time, so nothing piles up in git.
#
# Needs: gh (GH_TOKEN, GH_REPO), gpg, createrepo_c, rpmsign (the rpm package),
# apt-ftparchive (apt-utils), and the signing key's private half, armored, in
# PACKAGE_SIGNING_KEY. The release workflow runs this (publish.yml).
set -euo pipefail
cd "$(dirname "$0")/.."

repo=${GH_REPO:?set GH_REPO, e.g. crgklr/mychronix}
owner=${repo%%/*}
name=${repo#*/}
url="https://${owner,,}.github.io/$name"
keep=${KEEP_RELEASES:-3}
site=target/site
rm -rf "$site"
mkdir -p "$site/rpm" "$site/deb"

# The key, in a keyring of its own.
export GNUPGHOME
GNUPGHOME=$(mktemp -d)
trap 'rm -rf "$GNUPGHOME"' EXIT
printf '%s\n' "${PACKAGE_SIGNING_KEY:?set PACKAGE_SIGNING_KEY}" | gpg --batch --quiet --import
fpr=$(gpg --list-secret-keys --with-colons | awk -F: '/^fpr/ {print $10; exit}')
gpg --armor --export "$fpr" > "$site/mychronix.asc"
gpg --export "$fpr" > "$site/mychronix.gpg"

# The packages from the newest releases (not drafts or pre-releases).
tags=$(gh release list --exclude-drafts --exclude-pre-releases --limit "$keep" --json tagName -q '.[].tagName')
[ -n "$tags" ] || { echo "no releases to publish"; exit 1; }
for tag in $tags; do
  gh release download "$tag" --pattern '*.rpm' --dir "$site/rpm" --skip-existing
  gh release download "$tag" --pattern '*.deb' --dir "$site/deb" --skip-existing
done
latest=$(echo "$tags" | head -1)

# dnf: signed packages, and signed repository metadata.
cat > "$HOME/.rpmmacros" <<EOF
%_gpg_name $fpr
%_openpgp_sign_id $fpr
EOF
rpmsign --addsign "$site"/rpm/*.rpm
createrepo_c --quiet "$site/rpm"
gpg --batch --yes --detach-sign --armor "$site/rpm/repodata/repomd.xml"
cat > "$site/mychronix.repo" <<EOF
[mychronix]
name=Mychronix
baseurl=$url/rpm
enabled=1
gpgcheck=1
repo_gpgcheck=1
gpgkey=$url/mychronix.asc
EOF

# apt: a flat repository ("deb … ./"), its Release signed both ways.
(
  cd "$site/deb"
  apt-ftparchive packages . > Packages
  gzip -9kn Packages
  apt-ftparchive -o APT::FTPArchive::Release::Origin=Mychronix -o APT::FTPArchive::Release::Label=Mychronix release . > Release
  gpg --batch --yes --clearsign -o InRelease Release
  gpg --batch --yes --detach-sign --armor -o Release.gpg Release
)

# A page for anyone who opens the address in a browser.
cat > "$site/index.html" <<EOF
<!doctype html>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Mychronix packages</title>
<style>
  body { font: 16px/1.5 system-ui, sans-serif; max-width: 46rem; margin: 2rem auto; padding: 0 1rem; color: #1f2328; background: #fff }
  pre { background: #f6f8fa; padding: .8rem 1rem; border-radius: 6px; overflow-x: auto }
  @media (prefers-color-scheme: dark) { body { color: #e6edf3; background: #0d1117 } pre { background: #161b22 } a { color: #4493f8 } }
</style>
<h1>Mychronix packages</h1>
<p>Repositories for <a href="https://github.com/$repo">Mychronix</a> (newest: ${latest#v}). Add one once, and your system's updates bring each new version.</p>
<h2>Fedora</h2>
<pre>sudo curl -fsSL -o /etc/yum.repos.d/mychronix.repo $url/mychronix.repo
sudo dnf install mychronix</pre>
<h2>Ubuntu, Debian, Mint, Pop!_OS</h2>
<pre>sudo install -d /etc/apt/keyrings
curl -fsSL $url/mychronix.gpg | sudo tee /etc/apt/keyrings/mychronix.gpg &gt; /dev/null
echo "deb [signed-by=/etc/apt/keyrings/mychronix.gpg] $url/deb ./" | sudo tee /etc/apt/sources.list.d/mychronix.list
sudo apt update &amp;&amp; sudo apt install mychronix</pre>
<p>Signing key fingerprint: <code>$fpr</code></p>
EOF
touch "$site/.nojekyll"
find "$site" -type f | sort
