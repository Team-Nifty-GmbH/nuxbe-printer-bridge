# FLUX <-> CUPS Print Server

A REST API server written in Rust that interfaces with CUPS (Common UNIX Printing System) to handle print jobs and retrieve printer information. It can receive print jobs via WebSocket and poll for new printers and print jobs.

## Features

- REST API endpoints:
    - `GET /printers` - List all available printers with details (name, description, location, make/model, supported paper sizes)
    - `POST /print?printer=<printer_name>` - Upload and print a file using the specified printer
    - `GET /check_printers` - Manually check for new printers
    - `GET /check_jobs` - Manually check for print jobs

- WebSocket integration:
    - Subscribes to a Laravel Reverb WebSocket channel for real-time print job notifications
    - Listens on the "private-FluxErp.Models.PrintJobs" channel for "PrintJobCreated" events

- Admin interface:
    - Web UI for configuration
    - Configuration settings for instance name, API endpoints, and authentication tokens
    - Buttons to trigger printer detection and job checking

## Requirements

- Rust (1.56 or newer)
- CUPS (installed and configured)
- WebSocket server (Laravel Reverb)

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

2. Dependencies in `Cargo.toml`:
```toml
[dependencies]
actix-web = "4"
actix-files = "0.6"
actix-multipart = "0.6"
futures = "0.3"
reqwest = { version = "0.11", features = ["json"] }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
dirs = "5.0"
tokio = { version = "1", features = ["full", "macros", "rt-multi-thread"] }
tokio-tungstenite = { version = "0.20", features = ["native-tls"] }
tempfile = "3.8"
url = "2.4"
```

3. Compile the application:

```bash
cargo build --release
```

## Configuration

The application stores its configuration in `~/.config/flux-spooler/config.json`. The configuration includes:

- `instance_name`: Name for this printer server instance
- `host_url`: Base URL for API endpoints
- `printer_check_interval`: Interval in minutes to check for new printers
- `job_check_interval`: Interval in minutes to check for print jobs
- `notification_token`: Authentication token for printer notifications
- `print_jobs_token`: Authentication token for print jobs
- `admin_port`: Port for the admin interface
- `api_port`: Port for the API
- `websocket_url`: WebSocket URL for real-time job notifications
- `websocket_auth_token`: Authentication token for WebSocket

You can modify these settings using the admin interface or by directly editing the configuration file.

## Usage

### Running Manually

1. Start the server:
```bash
./target/release/flux-rust-spooler
```

By default, the server runs:
- API server on http://127.0.0.1:8080
- Admin interface on http://127.0.0.1:8081

### Setting up as a System Service (Linux)

1. Create a systemd service file:
```bash
sudo nano /etc/systemd/system/flux-rust-spooler.service
```

2. Paste the following content, replacing the placeholders with your values:
```ini
[Unit]
Description=FLUX <-> CUPS Print Server
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

## WebSocket Integration

The application connects to a WebSocket server to receive real-time print job notifications. It listens for "PrintJobCreated" events on the "private-FluxErp.Models.PrintJobs" channel. When an event is received, it:

1. Checks if the job is for this print server instance
2. Fetches the file using the media ID
3. Sends the file to the specified printer using CUPS

## Admin Interface

The admin interface is available at http://127.0.0.1:8081 (or the configured admin port). It allows you to:

1. Configure server settings
2. Manually check for new printers
3. Manually check for print jobs
4. Reconnect to the WebSocket server

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

### Check for New Printers

```bash
curl http://127.0.0.1:8080/check_printers
```

### Check for Print Jobs

```bash
curl http://127.0.0.1:8080/check_jobs
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

### WebSocket Connection Issues

1. Check that the WebSocket URL is correctly configured
2. Ensure the WebSocket authentication token is valid
3. Verify that the connection is not being blocked by a firewall

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

## License

This project is licensed under the MIT License - see the LICENSE file for details.