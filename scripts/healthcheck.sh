#!/bin/bash

# Health check script for Docker and systemd

# Check if process is running
if ! pgrep -x "opinion-hft" > /dev/null; then
    echo "CRITICAL: Process not running"
    exit 2
fi

# Check if recent orders are being placed (last 5 minutes)
if [ -f "logs/opinion-hft.log" ]; then
    RECENT_ACTIVITY=$(tail -n 1000 logs/opinion-hft.log | grep -c "order submitted\|opportunity detected")

    if [ "$RECENT_ACTIVITY" -eq 0 ]; then
        echo "WARNING: No recent trading activity detected"
        exit 1
    fi
fi

# All checks passed
echo "OK: Trading bot operational"
exit 0
