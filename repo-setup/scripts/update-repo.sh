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

# Regenerate Packages files
cd ${REPO_DIR}
sudo dpkg-scanpackages pool/main /dev/null | sudo tee dists/stable/main/binary-amd64/Packages > /dev/null
sudo dpkg-scanpackages pool/main /dev/null | sudo tee dists/stable/main/binary-armhf/Packages > /dev/null
sudo dpkg-scanpackages pool/main /dev/null | sudo tee dists/stable/main/binary-arm64/Packages > /dev/null

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
Description: Debian repository for rust-spooler by Team Nifty GmbH
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

# Sign Release file if GPG key exists
if [ -f /tmp/gpg-keyid.env ]; then
    source /tmp/gpg-keyid.env
    if [ -n "$KEYID" ]; then
        sudo gpg --armor --detach-sign --sign --default-key $KEYID -o Release.gpg Release
        sudo gpg --clearsign --default-key $KEYID -o InRelease Release
        echo "Release files signed"
    fi
fi

# Set permissions
sudo chown -R www-data:www-data ${REPO_DIR}
sudo chmod -R 755 ${REPO_DIR}

echo "Repository updated successfully!"
