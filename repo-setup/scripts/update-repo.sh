#!/bin/bash
# Script to update repository with new .deb packages
# Called by GitHub Actions after uploading new packages

set -e

REPO_DIR="/var/www/debian-repo"
PACKAGE_DIR="/tmp"

echo "Updating Debian repository..."

# Move new packages to pool
if ls ${PACKAGE_DIR}/*.deb 1>/dev/null 2>&1; then
    sudo mv ${PACKAGE_DIR}/*.deb ${REPO_DIR}/pool/main/
    echo "New packages moved to pool"
else
    echo "No new packages found in ${PACKAGE_DIR}"
    exit 0
fi

# Regenerate Packages files (filtered by architecture)
cd ${REPO_DIR}

# Generate Packages for each architecture by filtering dpkg-scanpackages output
# Use -m flag to include all versions/architectures (multiversion mode)
for arch in amd64 armhf arm64; do
    sudo dpkg-scanpackages -m pool/main /dev/null 2>/dev/null | \
        awk -v arch="$arch" '
            BEGIN { RS=""; FS="\n"; ORS="\n\n" }
            /Architecture: / {
                for (i=1; i<=NF; i++) {
                    if ($i ~ /^Architecture: /) {
                        split($i, a, ": ")
                        if (a[2] == arch) { print; break }
                    }
                }
            }
        ' | sudo tee dists/stable/main/binary-${arch}/Packages > /dev/null
done

# Compress Packages files
sudo gzip -kf dists/stable/main/binary-amd64/Packages
sudo gzip -kf dists/stable/main/binary-armhf/Packages
sudo gzip -kf dists/stable/main/binary-arm64/Packages

# Regenerate Release file
cd ${REPO_DIR}/dists/stable

# Create base Release file
cat > /tmp/release << EOF
Origin: Team Nifty
Label: Team Nifty Repository
Suite: stable
Codename: stable
Version: 1.0
Architectures: amd64 armhf arm64
Components: main
Description: Debian repository for nuxbe-printer-bridge by Team Nifty GmbH
Date: $(date -Ru)
EOF

# Calculate checksums
{
    echo "MD5Sum:"
    find main -type f -print0 | while IFS= read -r -d '' f; do
        size=$(stat -c %s "$f")
        hash=$(md5sum "$f" | cut -d' ' -f1)
        printf " %s %16d %s\n" "$hash" "$size" "$f"
    done
    echo "SHA1:"
    find main -type f -print0 | while IFS= read -r -d '' f; do
        size=$(stat -c %s "$f")
        hash=$(sha1sum "$f" | cut -d' ' -f1)
        printf " %s %16d %s\n" "$hash" "$size" "$f"
    done
    echo "SHA256:"
    find main -type f -print0 | while IFS= read -r -d '' f; do
        size=$(stat -c %s "$f")
        hash=$(sha256sum "$f" | cut -d' ' -f1)
        printf " %s %16d %s\n" "$hash" "$size" "$f"
    done
} >> /tmp/release

sudo mv /tmp/release Release

# Sign Release file with GPG
# The GPG key should be in root's keyring (since we run with sudo)
KEYID=$(sudo gpg --list-secret-keys --keyid-format LONG 2>/dev/null | grep -A 1 "sec" | grep -oP "rsa4096/\K[A-F0-9]+" | head -1)

if [ -n "$KEYID" ]; then
    sudo rm -f Release.gpg InRelease
    sudo gpg --batch --yes --armor --detach-sign --default-key $KEYID -o Release.gpg Release
    sudo gpg --batch --yes --clearsign --default-key $KEYID -o InRelease Release
    echo "Release files signed with key $KEYID"
else
    echo "ERROR: No GPG key found! Repository will not be usable."
    echo "Run generate-gpg-key.sh as root to create the signing key."
    exit 1
fi

# Set permissions
sudo chown -R www-data:www-data ${REPO_DIR}
sudo chmod -R 755 ${REPO_DIR}

echo "Repository updated successfully!"
