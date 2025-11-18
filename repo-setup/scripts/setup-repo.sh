#!/bin/bash
# Script to create and maintain Debian repository structure

set -e

REPO_DIR="/var/www/debian-repo"
KEYID=""

echo "Setting up Debian repository..."

# Create repository structure
sudo mkdir -p ${REPO_DIR}/{dists/stable/{main/binary-amd64,main/binary-armhf,main/binary-arm64},pool/main}

# Copy .deb files to pool
sudo cp *.deb ${REPO_DIR}/pool/main/

# Generate Packages files
cd ${REPO_DIR}
sudo dpkg-scanpackages pool/main /dev/null | sudo tee dists/stable/main/binary-amd64/Packages
sudo dpkg-scanpackages pool/main /dev/null | sudo tee dists/stable/main/binary-armhf/Packages
sudo dpkg-scanpackages pool/main /dev/null | sudo tee dists/stable/main/binary-arm64/Packages

# Compress Packages files
sudo gzip -k dists/stable/main/binary-amd64/Packages
sudo gzip -k dists/stable/main/binary-armhf/Packages
sudo gzip -k dists/stable/main/binary-arm64/Packages

# Generate Release file
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

# Calculate checksums in proper APT format (size + hash + path)
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

# Sign the Release file if GPG key is available
if [ -n "$KEYID" ]; then
    sudo gpg --armor --detach-sign --sign --default-key $KEYID Release
    sudo gpg --clearsign --default-key $KEYID Release
fi

# Set proper permissions
sudo chown -R www-data:www-data ${REPO_DIR}
sudo chmod -R 755 ${REPO_DIR}

echo "Repository setup complete!"
echo "Repository available at: ${REPO_DIR}"