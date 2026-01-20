 # Unpack
  mkdir -p "$HOME/opinion-hft"
  tar -xzf "artifacts/opinion-hft-linux-x86_64.tar.gz" -C "$HOME/opinion-hft"

  # Install layout + binary
  sudo bash "$HOME/opinion-hft/scripts/install.sh" "/opt/opinion-hft"
  sudo install -d "/opt/opinion-hft/bin"
  sudo install -m 0755 "$HOME/opinion-hft/target/x86_64-unknown-linux-gnu/release/opinion-hft" "/opt/opinion-hft/bin/opinion-hft"

  # Allow current user to write logs/data
  sudo chown -R "$USER:$USER" "/opt/opinion-hft"

  # .env (place here for dotenvy auto-load)
  cat > "/opt/opinion-hft/.env" <<'EOF'
  OPINION_API_KEY=your_api_key_here
  OPINION_PRIVATE_KEY=0x...
  OPINION_EOA_ADDRESS=0x...
  OPINION_ENV=aws
  OPINION_SCAN_ONLY=true
  OPINION_REST_URL=https://proxy.opinion.trade:8443/openapi/
  OPINION_WS_URL=wss://ws.opinion.trade
  BSC_RPC_URL=https://bsc-dataseed.binance.org
  OPINION_DATA_ENABLED=true
  OPINION_DATA_DB_PATH=/opt/opinion-hft/data/opinion-hft.sqlite
  OPINION_LOG_DIR=/opt/opinion-hft/logs
  OPINION_LOG_FILE_ENABLED=true
  RUST_LOG=info
  EOF

  # Start (scan-only, recording enabled)
  cd "/opt/opinion-hft"
  nohup "/opt/opinion-hft/bin/opinion-hft" > "/opt/opinion-hft/logs/opinion-hft.out" 2>&1 &


   If you keep /opt/opinion-hft:

  sudo install -d "/opt/opinion-hft/data" "/opt/opinion-hft/logs"
  sudo chown -R "$USER:$USER" "/opt/opinion-hft"

  Then place .env at /opt/opinion-hft/.env and start the bot from that directory:

  cd "/opt/opinion-hft"
  "/opt/opinion-hft/bin/opinion-hft"

  If you want no sudo, use your home directory and update the paths in .env:

  OPINION_DATA_DB_PATH=/home/ubuntu/opinion-hft/data/opinion-hft.sqlite
  OPINION_LOG_DIR=/home/ubuntu/opinion-hft/logs

  Then create directories without sudo:

  install -d "/home/ubuntu/opinion-hft/data" "/home/ubuntu/opinion-hft/logs"
