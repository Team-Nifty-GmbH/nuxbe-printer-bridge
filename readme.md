# Nuxbe Printer Bridge

A Rust application that bridges between Nuxbe ERP and CUPS (Common UNIX Printing System) to manage printers and handle print jobs. It provides a REST API, real-time WebSocket integration, and automatic synchronization between the systems.

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
  - Synchronization of printers with Nuxbe ERP API
  - Real-time printer updates and status tracking

- **Print Job Processing**:
  - Real-time print job notifications via Laravel Reverb WebSocket
  - Periodic polling for new print jobs when WebSocket is disabled
  - Automated download and printing of documents
  - Job status updates after printing

- **REST API Endpoints**:
  - `GET /printers` - List all available printers
  - `POST /print?printer=<printer_name>` - Upload and print a file
  - `GET /check_printers` - Manually trigger printer synchronization
  - `GET /check_jobs` - Manually check for pending print jobs

- **Configuration Options**:
  - Simple text-based configuration interface
  - Flexible WebSocket and polling configurations
  - Customizable update intervals

## Requirements

- Rust 2024 edition or newer
- CUPS installed and configured
- Network connection to Nuxbe ERP instance
- Laravel Reverb for WebSocket functionality (optional)

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
./target/release/rust-spooler config
```

The configuration is stored in `~/.config/nuxbe-printer-bridge/config.json` and includes:

- `instance_name`: Unique identifier for this print server
- `printer_check_interval`: How often to check for printer changes (minutes)
- `job_check_interval`: How often to check for print jobs (minutes)
- `nuxbe_url`: Base URL for the Nuxbe ERP API
- `nuxbe_api_token`: Authentication token for the API
- `api_port`: Port to run the API server on
- `reverb_disabled`: Whether to disable WebSocket and use polling instead
- `reverb_*` settings: Configuration for Laravel Reverb WebSocket connection

## Usage

### Running the Application

Start the application with:

```bash
./target/release/nuxbe-printer-bridge run
```

Or if installed via APT:
```bash
nuxbe-printer-bridge run
```

By default, the application will:
1. Detect all available CUPS printers
2. Synchronize printers with the Nuxbe ERP system
3. Begin listening for print jobs via WebSocket or polling
4. Start the REST API server on the configured port

### Printer Synchronization Flow

The application follows this order for printer synchronization:

1. Check for printers via CUPS
2. Load saved printers from printer.json
3. Create new printers in the API with POST requests to `/api/printers`
4. Get updated printer list with IDs from the API via GET to `/api/printers`
5. Delete removed printers from the API with DELETE to `/api/printers/{printer_id}`
6. Update changed printers in the API with PUT to `/api/printers/{printer_id}`

All API requests include the `instance_name` in the request body when required.

### Print Job Flow

For print jobs, the application:

1. Receives job notifications via WebSocket or polling
2. Validates that the job is for this print server instance
3. Downloads the file to be printed using the media ID
4. Prints the file on the appropriate printer
5. Updates the job status to `is_printed = true` via PUT to `/api/print-jobs/{job_id}`

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
ExecStart=/path/to/your/nuxbe-printer-bridge run
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

## API Usage Examples

### List All Printers

```bash
curl http://localhost:8080/printers
```

### Print a File

```bash
curl -X POST -F "file=@/path/to/document.pdf" "http://localhost:8080/print?printer=MyPrinter"
```

### Check for New Printers

```bash
curl http://localhost:8080/check_printers
```

### Check for New Print Jobs

```bash
curl http://localhost:8080/check_jobs
```

## Laravel Reverb Integration

The application uses Laravel Reverb for real-time print job notifications. It listens for "PrintJobCreated" events on the appropriate channel.

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
4. Test a manual job check with `curl http://localhost:8080/check_jobs`
5. Check CUPS logs for printing errors:
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