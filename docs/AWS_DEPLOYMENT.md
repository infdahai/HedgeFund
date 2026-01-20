# AWS Deployment (Ubuntu 24.04 x86_64)

This guide focuses on Ubuntu 24.04 on AWS. Notes for Amazon Linux are included where relevant.

## Build Prerequisites

Ubuntu 24.04:
```bash
sudo apt-get update
sudo apt-get install -y build-essential cmake pkg-config clang libclang-dev
```

Amazon Linux (dnf/yum):
```bash
sudo dnf install -y gcc gcc-c++ make cmake pkgconfig clang llvm-devel
```

Notes:
- `rquest` pulls in BoringSSL (`boring-sys2`) which uses `cmake` and `libclang` via `bindgen`.
- `rusqlite` is built with the bundled SQLite C library, so a C/C++ toolchain is required.
- `reqwest` uses Rustls in this repo, so OpenSSL is not required unless you change features.

## Build

```bash
cargo build --release
```

## Install Layout (Example)

```bash
sudo install -d "/opt/opinion-hft/bin" "/opt/opinion-hft/data"
sudo install -m 0755 "target/release/opinion-hft" "/opt/opinion-hft/bin/opinion-hft"
sudo install -d "/etc/opinion-hft"
sudo cp "deploy/opinion-hft.service" "/etc/systemd/system/opinion-hft.service"
```

## Environment File

Create `/etc/opinion-hft/opinion-hft.env`:

```bash
OPINION_API_KEY=your_api_key_here
OPINION_REST_URL=https://proxy.opinion.trade:8443/openapi/
OPINION_WS_URL=wss://ws.opinion.trade
OPINION_ENV=aws
OPINION_SCAN_ONLY=true

# Data collection
OPINION_DATA_DB_PATH=/opt/opinion-hft/data/opinion-hft.sqlite

# Logging to stdout (systemd journal)
OPINION_LOG_FILE_ENABLED=false
RUST_LOG=info,opinion_hft=debug
```

## Systemd Service

```bash
sudo systemctl daemon-reload
sudo systemctl enable --now opinion-hft
sudo systemctl status opinion-hft
sudo journalctl -u opinion-hft -f
```

Adjust these paths in `deploy/opinion-hft.service` if you use a different layout:
- `WorkingDirectory`
- `ExecStart`
- `EnvironmentFile`

## File Logging (Optional)

If you want rolling file logs in production:

```bash
OPINION_LOG_FILE_ENABLED=true
OPINION_LOG_DIR=/var/log/opinion-hft
```

Ensure the service user can write to the log directory.
