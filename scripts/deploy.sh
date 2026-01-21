# Deployment scripts

#!/bin/bash
set -e

# Color codes for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

echo -e "${GREEN}Starting Opinion HFT Bot deployment...${NC}"

# Check if required files exist
if [ ! -f ".env.production" ]; then
    echo -e "${RED}Error: .env.production file not found!${NC}"
    exit 1
fi

# Stop existing process
if pgrep -x "opinion-hft" > /dev/null; then
    echo -e "${YELLOW}Stopping existing process...${NC}"
    pkill -x "opinion-hft"
    sleep 5
fi

# Backup logs
if [ -d "logs" ]; then
    echo "Archiving previous logs..."
    tar -czf logs_backup_$(date +%Y%m%d_%H%M%S).tar.gz logs/
fi

# Start process with production environment
echo -e "${GREEN}Starting opinion-hft...${NC}"
nohup ./opinion-hft > logs/opinion-hft.log 2>&1 &

PID=$!
echo $PID > opinion-hft.pid

# Wait and check if process is running
sleep 3
if ps -p $PID > /dev/null; then
    echo -e "${GREEN}Successfully started (PID: $PID)${NC}"
    echo "Logs: tail -f logs/opinion-hft.log"
    exit 0
else
    echo -e "${RED}Failed to start. Check logs for details.${NC}"
    exit 1
fi
