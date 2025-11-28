#!/bin/bash
# Build script for creating .deb package on Debian/Ubuntu VPS

set -e

PACKAGE_NAME="nuxbe-printer-bridge"
VERSION="0.1.0"

echo "Building ${PACKAGE_NAME} v${VERSION} .deb package..."

# Install build dependencies
sudo apt update
sudo apt install -y debhelper devscripts build-essential libssl-dev pkg-config

# Install Rust using the recommended method
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
# shellcheck source=/dev/null
source ~/.cargo/env

# Build the package
cd /opt/nuxbe-printer-bridge
dpkg-buildpackage -us -uc -b

echo "Package built successfully!"
ls -la ../*.deb