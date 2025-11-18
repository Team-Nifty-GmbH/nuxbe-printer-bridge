#!/bin/bash
# Generate GPG key for signing Debian packages

set -e

echo "🔐 Generating GPG key for package signing..."

# Check if GPG key already exists
if gpg --list-secret-keys | grep -q "rust-spooler"; then
    echo "✅ GPG key already exists"
    exit 0
fi

# Generate GPG key non-interactively
cat > /tmp/gpg-key-params << EOF
%echo Generating GPG key for rust-spooler repository
Key-Type: RSA
Key-Length: 4096
Subkey-Type: RSA
Subkey-Length: 4096
Name-Real: Team Nifty Repository
Name-Email: packages@team-nifty.com
Expire-Date: 2y
Passphrase:
%commit
%echo done
EOF

gpg --batch --generate-key /tmp/gpg-key-params
rm /tmp/gpg-key-params

# Get the key ID
KEYID=$(gpg --list-secret-keys --keyid-format LONG | grep -A 1 "rust-spooler" | grep sec | awk '{print $2}' | cut -d'/' -f2)

echo "✅ GPG key generated successfully!"
echo "Key ID: $KEYID"

# Export public key
gpg --armor --export $KEYID > /tmp/repository-key.gpg
echo "📤 Public key exported to /tmp/repository-key.gpg"

# Create installation instructions
cat > /tmp/install-instructions.txt << EOF
To use this repository, users need to:

1. Download and add the GPG key:
   wget -qO - https://apt.team-nifty.com/repository-key.gpg | sudo apt-key add -

2. Add the repository to sources.list:
   echo "deb https://apt.team-nifty.com/ stable main" | sudo tee /etc/apt/sources.list.d/rust-spooler.list

3. Update package lists:
   sudo apt update

4. Install rust-spooler:
   sudo apt install rust-spooler
EOF

echo "📋 Installation instructions written to /tmp/install-instructions.txt"

# Save key ID for other scripts
echo "KEYID=$KEYID" > /tmp/gpg-keyid.env

echo "🔐 GPG setup complete!"