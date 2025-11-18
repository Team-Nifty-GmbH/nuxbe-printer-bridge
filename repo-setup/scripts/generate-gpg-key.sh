#!/bin/bash
# Generate GPG key for signing Debian packages

set -e

echo "🔐 Generating GPG key for package signing..."

# Check if GPG key already exists
if gpg --list-secret-keys | grep -q "Team Nifty"; then
    echo "✅ GPG key already exists"
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

echo "✅ GPG key generated successfully!"
echo "Key ID: $KEYID"

# Export public key
gpg --armor --export $KEYID > /tmp/repository-key.gpg
echo "📤 Public key exported to /tmp/repository-key.gpg"

# Create installation instructions
cat > /tmp/install-instructions.txt << EOF
To use this repository, users need to:

1. Download and add the GPG key:
   curl -fsSL https://apt.team-nifty.com/repository-key.gpg | sudo gpg --dearmor -o /usr/share/keyrings/team-nifty.gpg

2. Add the repository to sources.list:
   echo "deb [signed-by=/usr/share/keyrings/team-nifty.gpg] https://apt.team-nifty.com/ stable main" | sudo tee /etc/apt/sources.list.d/nuxbe-printer-bridge.list

3. Update package lists:
   sudo apt update

4. Install nuxbe-printer-bridge:
   sudo apt install nuxbe-printer-bridge
EOF

echo "📋 Installation instructions written to /tmp/install-instructions.txt"

# Save key ID for other scripts
echo "KEYID=$KEYID" > /tmp/gpg-keyid.env

echo "🔐 GPG setup complete!"