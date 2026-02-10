# Nuxbe Printer Bridge

A Rust application that bridges between Nuxbe ERP and CUPS (Common UNIX Printing System) to manage printers and handle print jobs. It provides real-time WebSocket integration, CLI printing, and automatic synchronization between the systems.

## Installation from APT Repository

```bash
# Add GPG key
curl -fsSL https://apt.team-nifty.com/repository-key.gpg | sudo gpg --dearmor -o /usr/share/keyrings/team-nifty.gpg

# Add repository
echo "deb [signed-by=/usr/share/keyrings/team-nifty.gpg] https://apt.team-nifty.com/ stable main" | sudo tee /etc/apt/sources.list.d/nuxbe-printer-bridge.list

# Install
sudo apt update
sudo apt install nuxbe-printer-bridge
```

## Features

- **Printer Management**:
  - Automatic discovery of local CUPS printers
  - Media size (paper format) detection via `lpoptions` for each printer
  - Synchronization of printers with Nuxbe ERP API using stable `system_name` identification
  - Two-pass matching: by `system_name` first, then by display `name` for legacy printers
  - Automatic URI, media size, and system name propagation to the ERP

- **Print Job Processing**:
  - Real-time print job notifications via Laravel Reverb WebSocket
  - Periodic polling for new print jobs when WebSocket is disabled
  - Automated download and printing of documents
  - Job status updates after printing

- **CLI Printing**:
  - Print local files directly from command line
  - Fetch and print jobs from the API by ID
  - List available printers
  - Custom job names

- **Configuration Options**:
  - TUI-based configuration editor
  - Flexible WebSocket and polling configurations
  - Customizable check intervals for printers and jobs

## Requirements

- CUPS installed and configured
- Network connection to Nuxbe ERP instance
- Laravel Reverb for WebSocket functionality (optional)
- Rust 2024 edition (only for building from source)

## Installation

### Dependencies

First, ensure CUPS is installed on your system:

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

### Building from Source

1. Clone the repository:
```bash
git clone https://github.com/Team-Nifty-GmbH/nuxbe-printer-bridge
cd nuxbe-printer-bridge
```

2. Build the application:
```bash
cargo build --release
```

## Configuration

The application can be configured using the built-in configuration tool:

```bash
nuxbe-printer-bridge config
```

The configuration is stored in `~/.config/nuxbe-printer-bridge/config.json` and includes:

- `instance_name`: Unique identifier for this print server (used as `spooler_name` in the API)
- `printer_check_interval`: How often to check for printer changes (minutes)
- `job_check_interval`: How often to check for print jobs (minutes)
- `flux_url`: Base URL for the Nuxbe ERP API
- `flux_api_token`: Sanctum Bearer token for API authentication
- `api_port`: Local API port (default: 8080)
- `reverb_disabled`: Whether to disable WebSocket and use polling instead
- `reverb_app_id`, `reverb_app_key`, `reverb_app_secret`: Laravel Reverb credentials
- `reverb_use_tls`: Whether to use WSS (secure WebSocket)
- `reverb_host`: Reverb server hostname
- `reverb_auth_endpoint`: Broadcasting auth URL

## Usage

### Running the Server

Start the background service:

```bash
nuxbe-printer-bridge run
```

With verbose logging:
```bash
nuxbe-printer-bridge -v run      # info level
nuxbe-printer-bridge -vv run     # debug level
nuxbe-printer-bridge -vvv run    # trace level
```

The server will:
1. Detect all available CUPS printers
2. Synchronize printers with the Nuxbe ERP system
3. Listen for print jobs via WebSocket or polling

### CLI Commands

**List available printers:**
```bash
nuxbe-printer-bridge printers
```

**Print a file:**
```bash
# Print to default printer
nuxbe-printer-bridge print -f /path/to/document.pdf

# Print to specific printer
nuxbe-printer-bridge print -f /path/to/document.pdf -p "My Printer"

# Print with custom job name
nuxbe-printer-bridge print -f /path/to/document.pdf -n "Invoice #123"

# Fetch and print a job from the API by ID
nuxbe-printer-bridge print --job 123
```

**Configure settings:**
```bash
nuxbe-printer-bridge config
```

### Printer Synchronization Flow

The application follows this order for printer synchronization:

1. Discover local CUPS printers and query supported media sizes via `lpoptions -p <name> -l`
2. Load saved printers from `printers.json`
3. Fetch API printers filtered by `spooler_name` (the configured `instance_name`)
4. Match local printers to API printers using two-pass matching:
   - **Pass 1**: Match by `system_name` (stable CUPS identifier)
   - **Pass 2**: Fall back to matching by display `name` for legacy printers where `system_name` is null
5. Create new printers in the API (POST `/api/printers`)
6. Delete removed printers from the API (DELETE `/api/printers/{id}`)
7. Update changed printers in the API (PUT `/api/printers` with ID in body), including legacy-matched printers that need `system_name`, `uri`, and `media_sizes` populated

All API requests include the `instance_name` as `spooler_name` in the request body.

### Print Job Flow

For print jobs, the application:

1. Receives job notifications via WebSocket (`PrintJobCreated` event) or periodic polling
2. On WebSocket connect, fetches any pending jobs created while offline
3. Fetches full job details from the API (GET `/api/print-jobs/{id}?include=printer`)
4. Downloads the document via media ID (GET `/api/media/private/{media_id}`)
5. Prints the file on the appropriate CUPS printer (falls back to default if specified printer not found)
6. Marks the job as completed (PUT `/api/print-jobs` with ID in body, `is_completed: true`)

### Setting up as a System Service (Linux)

1. Create a systemd service file:
```bash
sudo nano /etc/systemd/system/nuxbe-printer-bridge.service
```

2. Add the following content:
```ini
[Unit]
Description=Nuxbe Printer Bridge
After=network.target cups.service
Requires=cups.service

[Service]
Type=simple
User=<your-username>
ExecStart=/usr/bin/nuxbe-printer-bridge run
Restart=on-failure
RestartSec=5
StandardOutput=journal
StandardError=journal

[Install]
WantedBy=multi-user.target
```

3. Enable and start the service:
```bash
sudo systemctl daemon-reload
sudo systemctl enable nuxbe-printer-bridge.service
sudo systemctl start nuxbe-printer-bridge.service
```

## Laravel Reverb Integration

The application uses Laravel Reverb for real-time print job notifications. It subscribes to the `private-print_job.` channel and listens for `.PrintJobCreated` events.

When a print job event is received, the application:
1. Extracts the job ID from the event payload
2. Fetches the full job details from the API
3. Processes and prints the job

On initial WebSocket connection, the application automatically fetches any pending jobs from the API to process jobs that were created while the application was offline.

The WebSocket connection automatically reconnects if it fails, with a configurable delay between reconnection attempts.

## Troubleshooting

### Empty Printer List

If you don't see any printers:

1. Check if CUPS is running:
```bash
systemctl status cups
```

2. Verify that printers are visible to CUPS:
```bash
lpstat -a
lpstat -p
lpstat -v
```

3. Check application logs for API connection errors

### Print Jobs Not Processing

1. Ensure your instance_name is correctly configured
2. Check that the API token has the necessary permissions
3. Verify printer IDs match between the API and local system
4. Check CUPS logs for printing errors:
```bash
sudo journalctl -u cups.service
```

### WebSocket Connection Issues

1. Verify Reverb configuration settings
2. Check for firewall blocking WebSocket connections
3. Consider enabling polling by setting `reverb_disabled` to true

## License

This project is licensed under the MIT License - see the LICENSE file for details.

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.
