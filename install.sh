#!/bin/bash

# Raspberry Pi Zero 2 W Installation Script
# This script installs: cups, cups-tools, openssh, git, mc, nano
# and optionally rust

# Colors for better readability
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

echo -e "${BLUE}Raspberry Pi Zero 2 W Setup Script${NC}"
echo -e "${YELLOW}This script will install the following packages:${NC}"
echo "- CUPS (Printing System)"
echo "- CUPS tools"
echo "- Dependencies for Flux Spooler"
echo "- Tailscale (optional)"
echo "- Rust (optional)"
echo ""

# Update system packages
echo -e "${GREEN}Updating package lists and upgrading existing packages...${NC}"
sudo apt update && sudo apt upgrade -y && sudo apt autoremove -y

# Install the required packages
echo -e "${GREEN}Installing CUPS, CUPS-Tools, OpenSSH, Git, MC, and Nano...${NC}"
sudo apt install -y cups cups-client cups-bsd git mc nano libssl-dev pkg-config tmux

# Enable and start services1
echo -e "${GREEN}Enabling and starting services...${NC}"
sudo systemctl enable --now cups

# Optional: Install Tailscale
read -p "Would you like to install Tailscale? (y/n): " install_tailscale
if [[ $install_tailscale == "y" || $install_tailscale == "Y" ]]; then
    echo -e "${GREEN}Installing Tailscale...${NC}"
    # Add Tailscale's GPG key and repository
    curl -fsSL https://pkgs.tailscale.com/stable/raspbian/bookworm.noarmor.gpg | sudo tee /usr/share/keyrings/tailscale-archive-keyring.gpg >/dev/null
    curl -fsSL https://pkgs.tailscale.com/stable/raspbian/bookworm.tailscale-keyring.list | sudo tee /etc/apt/sources.list.d/tailscale.list

    # Update repositories and install Tailscale
    sudo apt-get update
    sudo apt-get install -y tailscale

    echo -e "${GREEN}Tailscale has been installed.${NC}"

    # Ask if user wants to connect to Tailscale network now
    read -p "Would you like to connect to your Tailscale network now? (y/n): " connect_tailscale
    if [[ $connect_tailscale == "y" || $connect_tailscale == "Y" ]]; then
        echo -e "${YELLOW}Connecting to Tailscale network...${NC}"
        echo -e "${YELLOW}Follow the instructions to authenticate:${NC}"
        sudo tailscale up
        echo -e "${GREEN}Tailscale connection established!${NC}"
    else
        echo -e "${YELLOW}You can connect later with:${NC} sudo tailscale up"
    fi
fi

# Optional: Install Rust
read -p "Would you like to install Rust? (y/n): " install_rust
if [[ $install_rust == "y" || $install_rust == "Y" ]]; then
    echo -e "${GREEN}Installing Rust...${NC}"
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

    # Add Rust to the current shell session
    source $HOME/.cargo/env

    echo -e "${GREEN}Rust has been installed.${NC}"

    # Verify Rust installation
    echo -e "${YELLOW}Verifying Rust installation:${NC}"
    rustc --version
    cargo --version
fi

# Display summary
echo -e "\n${BLUE}Installation Summary:${NC}"
echo -e "${GREEN}✓${NC} CUPS (Printing System)"
echo -e "${GREEN}✓${NC} CUPS Tools"
echo -e "${GREEN}✓${NC} Git"
echo -e "${GREEN}✓${NC} Midnight Commander (mc)"
echo -e "${GREEN}✓${NC} Nano Editor"

if [[ $install_rust == "y" || $install_rust == "Y" ]]; then
    echo -e "${GREEN}✓${NC} Rust Programming Language"
fi

if [[ $install_tailscale == "y" || $install_tailscale == "Y" ]]; then
    echo -e "${GREEN}✓${NC} Tailscale"
    if [[ $connect_tailscale == "y" || $connect_tailscale == "Y" ]]; then
        echo -e "Tailscale IP: $(tailscale ip -4)"
    fi
fi

# Import SSH keys from GitHub for specified users
echo -e "${GREEN}Importing SSH keys from GitHub...${NC}"

# Create SSH directory if it doesn't exist
mkdir -p ~/.ssh
chmod 700 ~/.ssh

# Download and add SSH keys from GitHub
echo -e "${BLUE}Fetching SSH keys for user slupi...${NC}"
curl -s https://github.com/slupi.keys >> ~/.ssh/authorized_keys

echo -e "${BLUE}Fetching SSH keys for user patrickweh...${NC}"
curl -s https://github.com/patrickweh.keys >> ~/.ssh/authorized_keys

chmod 600 ~/.ssh/authorized_keys
echo -e "${GREEN}✓${NC} SSH keys imported"

echo -e "\n${BLUE}Installation complete!${NC}"
echo -e "CUPS is accessible at http://$(hostname):631"
echo -e "${YELLOW}Remember to set a strong password if you haven't already:${NC}"
echo -e "passwd\n"