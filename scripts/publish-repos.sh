#!/bin/bash
# Builds Mychronix's dnf and apt repositories into target/site, for GitHub
# Pages: the .rpm and .deb from the newest few GitHub releases, signed, with
# the repository files and the website (scripts/build-site.py). Rebuilt from the
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

# The website, for anyone who opens the address in a browser.
date=$(gh release view "$latest" --json publishedAt -q .publishedAt)
python3 scripts/build-site.py "$site" --version "${latest#v}" --date "$(date -d "$date" '+%B %-d, %Y')" \
  --repo "$repo" --url "$url" --fpr "$fpr"
touch "$site/.nojekyll"
find "$site" -type f | sort
