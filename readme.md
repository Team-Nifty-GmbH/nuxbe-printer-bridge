# CUPS Print Server

A simple REST API server written in Rust that interfaces with CUPS (Common UNIX Printing System) to handle print jobs and retrieve printer information.

## Features

- `GET /printers` - List all available printers with details (name, description, location, make/model, supported paper sizes)
- `POST /print?printer=<printer_name>` - Upload and print a file using the specified printer

## Requirements

- Rust (1.56 or newer)
- CUPS (installed and configured)
- curl (for testing)

## Installation

### Dependencies

First, make sure you have CUPS installed on your system:

#### Ubuntu/Debian
```bash
sudo apt update
sudo apt install cups cups-client
```

#### Fedora/RHEL/CentOS
```bash
sudo dnf install cups cups-client
```

#### macOS
```bash
brew install cups
```

### Building the Application

1. Clone the repository:
```bash
git clone https://github.com/Team-Nifty-GmbH/flux-rust-spooler
cd flux-rust-spooler
```

2. Add the dependencies to your `Cargo.toml`:
```toml
[dependencies]
actix-web = "4.0"
actix-multipart = "0.4"
futures = "0.3"
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
tempfile = "3.2"
```

3. Compile the application:

#### For Linux (x86_64)
```bash
cargo build --release
```

#### For macOS (Intel x86_64)
```bash
cargo build --release
```

#### For macOS (Apple Silicon)
```bash
rustup target add aarch64-apple-darwin
cargo build --release --target aarch64-apple-darwin
```

## Usage

### Running Manually

1. Start the server:
```bash
./target/release/flux-rust-spooler
```

By default, the server runs on http://127.0.0.1:8080

### Setting up as a System Service (Linux)

1. Create a systemd service file:
```bash
sudo nano /etc/systemd/system/flux-rust-spooler.service
```

2. Paste the following content, replacing the placeholders with your values:
```ini
[Unit]
Description=CUPS Print Server
After=network.target cups.service
Requires=cups.service

[Service]
Type=simple
User=<your-username>
ExecStart=/path/to/your/flux-rust-spooler
WorkingDirectory=/path/to/your/project/directory
Restart=on-failure
RestartSec=5
StandardOutput=journal
StandardError=journal

# Hardening options
ProtectSystem=full
PrivateTmp=true
NoNewPrivileges=true

[Install]
WantedBy=multi-user.target
```

3. Enable and start the service:
```bash
sudo systemctl daemon-reload
sudo systemctl enable flux-rust-spooler.service
sudo systemctl start flux-rust-spooler.service
```

4. Check the status:
```bash
sudo systemctl status flux-rust-spooler.service
```

### Setting up as a Launch Agent (macOS)

1. Create a plist file in your LaunchAgents directory:
```bash
mkdir -p ~/Library/LaunchAgents
nano ~/Library/LaunchAgents/com.teamnifty.flux-rust-spooler.plist
```

2. Add the following content:
```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.teamnifty.flux-rust-spooler</string>
    <key>ProgramArguments</key>
    <array>
        <string>/path/to/your/flux-rust-spooler</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>StandardOutPath</key>
    <string>/tmp/flux-rust-spooler.log</string>
    <key>StandardErrorPath</key>
    <string>/tmp/flux-rust-spooler.log</string>
    <key>WorkingDirectory</key>
    <string>/path/to/your/project/directory</string>
</dict>
</plist>
```

3. Load the service:
```bash
launchctl load ~/Library/LaunchAgents/com.teamnifty.flux-rust-spooler.plist
```

## API Examples

### List All Printers

```bash
curl http://127.0.0.1:8080/printers
```

Example output:
```json
{
  "printers": [
    {
      "name": "HP_LaserJet_Pro_MFP",
      "description": "HP LaserJet Pro MFP",
      "location": "Office",
      "make_and_model": "HP LaserJet Pro MFP M428fdw",
      "media_sizes": ["A4", "Letter", "Legal", "Executive"]
    },
    {
      "name": "Brother_HL-L2340D",
      "description": "Brother Printer",
      "location": "Home",
      "make_and_model": "Brother HL-L2340D series",
      "media_sizes": ["A4", "Letter", "A5"]
    }
  ]
}
```

### Print a File

```bash
curl -X POST -F "file=@/path/to/document.pdf" "http://127.0.0.1:8080/print?printer=HP_LaserJet_Pro_MFP"
```

Example output:
```
Print job submitted: request id is HP_LaserJet_Pro_MFP-123 (1 file(s))
```

## Troubleshooting

### Empty Printer List

If the `/printers` endpoint returns an empty list:

1. Check if CUPS is running:
```bash
systemctl status cups  # Linux
brew services info cups  # macOS
```

2. Verify you can see printers with CUPS command:
```bash
lpstat -a
```

3. Ensure your user has permission to access CUPS:
```bash
sudo usermod -a -G lpadmin yourusername  # Linux
```

### Print Jobs Failing

1. Check CUPS logs:
```bash
sudo journalctl -u cups.service  # Linux
cat /var/log/cups/error_log  # macOS
```

2. Verify printer permissions:
```bash
sudo lpstat -t
```

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

## License

This project is licensed under the MIT License - see the LICENSE file for details.