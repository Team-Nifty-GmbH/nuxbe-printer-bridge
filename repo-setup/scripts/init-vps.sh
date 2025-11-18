#!/bin/bash
# Initial VPS setup script - run this on the VPS as root
# Usage: curl -sSL https://raw.githubusercontent.com/Team-Nifty-GmbH/rust-spooler/main/repo-setup/scripts/init-vps.sh | bash

set -e

VPS_IP="37.120.160.146"
DOMAIN="apt.team-nifty.com"

echo "=== Initial VPS Setup for Debian Repository ==="
echo "VPS IP: $VPS_IP"
echo "Domain: $DOMAIN"
echo ""

# Update system
echo ">>> Updating system packages..."
apt update && apt upgrade -y

# Install required packages
echo ">>> Installing required packages..."
apt install -y nginx git gpg debhelper devscripts build-essential libssl-dev pkg-config dpkg-dev curl

# Install Rust
echo ">>> Installing Rust..."
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
# shellcheck source=/dev/null
source ~/.cargo/env

# Create deploy user for GitHub Actions
echo ">>> Creating deploy user..."
if ! id "deploy" &>/dev/null; then
    useradd -m -s /bin/bash deploy
    usermod -aG sudo deploy
    echo "deploy ALL=(ALL) NOPASSWD: /opt/rust-spooler/repo-setup/scripts/update-repo.sh" >> /etc/sudoers.d/deploy
    chmod 440 /etc/sudoers.d/deploy
fi

# Set up SSH directory for deploy user
mkdir -p /home/deploy/.ssh
chmod 700 /home/deploy/.ssh
touch /home/deploy/.ssh/authorized_keys
chmod 600 /home/deploy/.ssh/authorized_keys
chown -R deploy:deploy /home/deploy/.ssh

echo ""
echo ">>> IMPORTANT: Add your GitHub Actions SSH public key to:"
echo "    /home/deploy/.ssh/authorized_keys"
echo ""

# Clone repository
echo ">>> Cloning rust-spooler repository..."
if [ ! -d /opt/rust-spooler ]; then
    git clone https://github.com/Team-Nifty-GmbH/rust-spooler.git /opt/rust-spooler
else
    cd /opt/rust-spooler && git pull
fi
chown -R root:root /opt/rust-spooler

# Create repository directory structure
echo ">>> Creating repository structure..."
mkdir -p /var/www/debian-repo/{dists/stable/{main/binary-amd64,main/binary-armhf,main/binary-arm64},pool/main}
chown -R www-data:www-data /var/www/debian-repo
chmod -R 755 /var/www/debian-repo

# Configure nginx
echo ">>> Configuring nginx..."
cp /opt/rust-spooler/repo-setup/nginx/debian-repo.conf /etc/nginx/sites-available/
ln -sf /etc/nginx/sites-available/debian-repo.conf /etc/nginx/sites-enabled/
rm -f /etc/nginx/sites-enabled/default

# Test nginx configuration
nginx -t

# Start nginx
systemctl enable --now nginx
systemctl reload nginx

# Generate GPG key for package signing
echo ">>> Generating GPG key..."
/opt/rust-spooler/repo-setup/scripts/generate-gpg-key.sh

# Copy public key to web root
if [ -f /tmp/repository-key.gpg ]; then
    cp /tmp/repository-key.gpg /var/www/debian-repo/
    chown www-data:www-data /var/www/debian-repo/repository-key.gpg
fi

echo ""
echo "=== Setup Complete ==="
echo ""
echo "Next steps:"
echo "1. Add your GitHub Actions SSH public key to /home/deploy/.ssh/authorized_keys"
echo "2. Set up SSL with: certbot --nginx -d $DOMAIN"
echo "3. Configure GitHub secrets:"
echo "   - DEPLOY_KEY: Your SSH private key"
echo "   - REPO_HOST: $VPS_IP"
echo ""
echo "Repository will be available at: https://$DOMAIN/"
