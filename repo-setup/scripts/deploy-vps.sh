#!/bin/bash
# Complete VPS deployment script for Debian repository

set -e

echo "🚀 Starting VPS deployment for nuxbe-printer-bridge Debian repository"

# Update system
echo "📦 Updating system packages..."
sudo apt update && sudo apt upgrade -y

# Install required packages
echo "📦 Installing required packages..."
sudo apt install -y nginx git gpg debhelper devscripts build-essential libssl-dev pkg-config

# Install Rust using recommended method
echo "🦀 Installing Rust..."
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
# shellcheck source=/dev/null
source ~/.cargo/env

# Clone repository
echo "📂 Cloning nuxbe-printer-bridge repository..."
cd /opt
sudo git clone https://github.com/Team-Nifty-GmbH/nuxbe-printer-bridge.git
sudo chown -R $USER:$USER /opt/nuxbe-printer-bridge

# Build the .deb package
echo "🔨 Building .deb package..."
cd /opt/nuxbe-printer-bridge
./repo-setup/scripts/build-deb.sh

# Set up GPG key for signing
echo "🔐 Setting up GPG key..."
./repo-setup/scripts/generate-gpg-key.sh

# Create repository structure
echo "📚 Setting up repository..."
./repo-setup/scripts/setup-repo.sh

# Configure nginx
echo "🌐 Configuring nginx..."
sudo cp repo-setup/nginx/debian-repo.conf /etc/nginx/sites-available/
sudo ln -sf /etc/nginx/sites-available/debian-repo.conf /etc/nginx/sites-enabled/
sudo rm -f /etc/nginx/sites-enabled/default

# Test nginx configuration
sudo nginx -t

# Start services
echo "▶️ Starting services..."
sudo systemctl enable --now nginx

echo "✅ Deployment complete!"
echo "Repository available at: https://apt.team-nifty.com/"
echo "Add to sources.list: deb https://apt.team-nifty.com/ stable main"