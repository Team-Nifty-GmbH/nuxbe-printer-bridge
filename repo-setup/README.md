# Debian Repository Setup

This directory contains everything needed to build and host a Debian repository for nuxbe-printer-bridge on a VPS.

## Quick Start

1. **Deploy to VPS** (Ubuntu/Debian):
   ```bash
   ./scripts/deploy-vps.sh
   ```

2. **Configure SSL** (recommended):
   - Set up Let's Encrypt with certbot for apt.team-nifty.com
   - Uncomment the HTTPS server block in nginx config

## Manual Steps

### 1. Build Package
```bash
./scripts/build-deb.sh
```

### 2. Generate GPG Key
```bash
./scripts/generate-gpg-key.sh
```

### 3. Setup Repository
```bash
./scripts/setup-repo.sh
```

### 4. Configure Nginx
```bash
sudo cp nginx/debian-repo.conf /etc/nginx/sites-available/
sudo ln -s /etc/nginx/sites-available/debian-repo.conf /etc/nginx/sites-enabled/
sudo systemctl reload nginx
```

## Repository Structure
```
/var/www/debian-repo/
├── dists/stable/
│   ├── Release
│   ├── Release.gpg
│   └── main/binary-amd64/
│       ├── Packages
│       └── Packages.gz
└── pool/main/
    └── rust-spooler_0.1.0-1_amd64.deb
```

## Client Usage

Users can install from your repository:

```bash
# Add GPG key (modern method)
curl -fsSL https://apt.team-nifty.com/repository-key.gpg | sudo gpg --dearmor -o /usr/share/keyrings/team-nifty.gpg

# Add repository
echo "deb [signed-by=/usr/share/keyrings/team-nifty.gpg] https://apt.team-nifty.com/ stable main" | sudo tee /etc/apt/sources.list.d/nuxbe-printer-bridge.list

# Install package
sudo apt update
sudo apt install nuxbe-printer-bridge
```

## Files Overview

- `scripts/build-deb.sh` - Builds .deb package on VPS
- `scripts/setup-repo.sh` - Creates repository structure
- `scripts/generate-gpg-key.sh` - Creates signing key
- `scripts/deploy-vps.sh` - Complete VPS deployment
- `nginx/debian-repo.conf` - Nginx configuration

## Security

- GPG keys are generated without passphrase for automation
- Nginx serves repository with proper headers
- Package signing ensures integrity