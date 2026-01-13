#!/bin/bash
# Monitor QBZ resource usage
# Usage: ./scripts/monitor-resources.sh

echo "=== QBZ Resource Monitor ==="
echo "Waiting for QBZ process..."
echo ""

# Wait for process to start
while ! pgrep -f "qbz-nix" > /dev/null 2>&1; do
    sleep 0.5
done

PID=$(pgrep -f "qbz-nix" | head -1)
echo "Found QBZ PID: $PID"
echo ""
echo "Press Ctrl+C to stop monitoring"
echo "==========================================="
echo ""

# Monitor loop
while true; do
    if ! ps -p "$PID" > /dev/null 2>&1; then
        echo "QBZ process ended"
        exit 0
    fi

    # Get stats
    CPU=$(ps -p "$PID" -o %cpu= 2>/dev/null | tr -d ' ')
    MEM=$(ps -p "$PID" -o rss= 2>/dev/null | tr -d ' ')
    MEM_MB=$((MEM / 1024))
    THREADS=$(ps -p "$PID" -o nlwp= 2>/dev/null | tr -d ' ')

    # Get WebKit process if exists
    WEBKIT_PID=$(pgrep -f "WebKitWebProcess" | head -1 2>/dev/null)
    if [[ -n "$WEBKIT_PID" ]]; then
        WEBKIT_MEM=$(ps -p "$WEBKIT_PID" -o rss= 2>/dev/null | tr -d ' ')
        WEBKIT_MB=$((WEBKIT_MEM / 1024))
        WEBKIT_CPU=$(ps -p "$WEBKIT_PID" -o %cpu= 2>/dev/null | tr -d ' ')
    else
        WEBKIT_MB=0
        WEBKIT_CPU=0
    fi

    TOTAL_MB=$((MEM_MB + WEBKIT_MB))

    # Print stats
    printf "\r[QBZ] CPU: %5s%% | RAM: %4dMB | Threads: %2s | [WebKit] CPU: %5s%% | RAM: %4dMB | TOTAL: %4dMB" \
        "$CPU" "$MEM_MB" "$THREADS" "$WEBKIT_CPU" "$WEBKIT_MB" "$TOTAL_MB"

    sleep 1
done
