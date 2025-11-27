#!/bin/bash
# Generate GPG key for signing Debian packages
# IMPORTANT: Run this script as root (sudo ./generate-gpg-key.sh)

set -e

# Check if running as root
if [ "$EUID" -ne 0 ]; then
    echo "ERROR: This script must be run as root (sudo $0)"
    exit 1
fi

echo "Generating GPG key for package signing..."

# Check if GPG key already exists
if gpg --list-secret-keys | grep -q "Team Nifty"; then
    echo "GPG key already exists"
    KEYID=$(gpg --list-secret-keys --keyid-format LONG | grep -A 1 "sec" | grep -oP "rsa4096/\K[A-F0-9]+" | head -1)
    echo "Key ID: $KEYID"
    exit 0
fi

# Generate GPG key non-interactively
cat > /tmp/gpg-key-params << EOF
%echo Generating GPG key for nuxbe-printer-bridge repository
Key-Type: RSA
Key-Length: 4096
Subkey-Type: RSA
Subkey-Length: 4096
Name-Real: Team Nifty Repository
Name-Email: packages@team-nifty.com
Expire-Date: 2y
%no-protection
%commit
%echo done
EOF

gpg --batch --generate-key /tmp/gpg-key-params
rm /tmp/gpg-key-params

# Get the key ID
KEYID=$(gpg --list-secret-keys --keyid-format LONG | grep -B 1 "Team Nifty" | grep sec | awk '{print $2}' | cut -d'/' -f2)

echo "GPG key generated successfully!"
echo "Key ID: $KEYID"

# Export public key to web directory
REPO_DIR="/var/www/debian-repo"
mkdir -p $REPO_DIR

gpg --armor --export $KEYID > $REPO_DIR/repository-key.gpg
chown www-data:www-data $REPO_DIR/repository-key.gpg
chmod 644 $REPO_DIR/repository-key.gpg
echo "Public key exported to $REPO_DIR/repository-key.gpg"

# Print installation instructions
cat << EOF

====================================
INSTALLATION INSTRUCTIONS FOR USERS
====================================

1. Download and add the GPG key:
   curl -fsSL https://apt.team-nifty.com/repository-key.gpg | sudo gpg --dearmor -o /usr/share/keyrings/team-nifty.gpg

2. Add the repository to sources.list:
   echo "deb [signed-by=/usr/share/keyrings/team-nifty.gpg] https://apt.team-nifty.com/ stable main" | sudo tee /etc/apt/sources.list.d/nuxbe-printer-bridge.list

3. Update package lists:
   sudo apt update

4. Install nuxbe-printer-bridge:
   sudo apt install nuxbe-printer-bridge

====================================
GPG setup complete!
EOF