#!/usr/bin/env bash
# test-connection.sh — Verify the Flipper MCP server is reachable and responding.
#
# Usage:
#   ./scripts/test-connection.sh                  # uses flipper-mcp.local
#   ./scripts/test-connection.sh 192.168.1.50     # use IP directly
set -euo pipefail

HOST="${1:-flipper-mcp.local}"
BASE="http://$HOST:8080"

PASS=0
FAIL=0

ok()   { echo "  [OK]  $*"; ((PASS++)) || true; }
fail() { echo "  [FAIL] $*"; ((FAIL++)) || true; }
hdr()  { echo ""; echo "── $* ──────────────────────────────────────"; }

echo "Testing Flipper MCP server at $BASE"

# ── 1. Health check ──────────────────────────────────────────────────────────
hdr "Health"
HEALTH=$(curl -sf --max-time 5 "$BASE/health" 2>/dev/null || true)
if [[ -z "$HEALTH" ]]; then
    fail "No response from $BASE/health  (is the device on the network?)"
    echo ""
    echo "Tip: check the IP with ./scripts/monitor.sh and look for 'IP:' in the log."
    exit 1
fi
ok "Server reachable"
echo "$HEALTH" | python3 -m json.tool 2>/dev/null || echo "  $HEALTH"

# ── 2. MCP initialize ────────────────────────────────────────────────────────
hdr "MCP Initialize"
INIT=$(curl -sf --max-time 5 -X POST "$BASE/mcp" \
  -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"curl-test","version":"1.0"}}}' \
  2>/dev/null || true)

if [[ -z "$INIT" ]]; then
    fail "No response to initialize"
else
    PROTO=$(echo "$INIT" | python3 -c "import json,sys; d=json.load(sys.stdin); print(d.get('result',{}).get('protocolVersion','?'))" 2>/dev/null || echo "?")
    ok "MCP handshake OK (protocolVersion=$PROTO)"
fi

# ── 3. Tools list ────────────────────────────────────────────────────────────
hdr "Available Tools"
TOOLS_JSON=$(curl -sf --max-time 10 -X POST "$BASE/mcp" \
  -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}' \
  2>/dev/null || true)

if [[ -z "$TOOLS_JSON" ]]; then
    fail "No response to tools/list"
else
    COUNT=$(echo "$TOOLS_JSON" | python3 -c \
      "import json,sys; d=json.load(sys.stdin); tools=d.get('result',{}).get('tools',[]); [print(f'  {t[\"name\"]}') for t in sorted(tools,key=lambda t:t['name'])]; print(f'\n  Total: {len(tools)} tools')" \
      2>/dev/null || echo "  (could not parse tool list)")
    ok "tools/list responded"
    echo "$COUNT"
fi

# ── 4. Basic tool call ───────────────────────────────────────────────────────
hdr "Tool Call: system_info"
SYSINFO=$(curl -sf --max-time 10 -X POST "$BASE/mcp" \
  -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"system_info","arguments":{}}}' \
  2>/dev/null || true)

if [[ -z "$SYSINFO" ]]; then
    fail "No response to system_info tool call"
else
    ERR=$(echo "$SYSINFO" | python3 -c "import json,sys; d=json.load(sys.stdin); print(d.get('error',{}).get('message',''))" 2>/dev/null || true)
    if [[ -n "$ERR" ]]; then
        fail "system_info returned error: $ERR"
    else
        ok "system_info tool call succeeded"
        echo "$SYSINFO" | python3 -c \
          "import json,sys; d=json.load(sys.stdin); content=d.get('result',{}).get('content',[]); [print('  ' + c.get('text','')) for c in content[:3]]" \
          2>/dev/null || true
    fi
fi

# ── Summary ──────────────────────────────────────────────────────────────────
echo ""
echo "────────────────────────────────────────────────"
echo "Results: $PASS passed, $FAIL failed"

if [[ $FAIL -eq 0 ]]; then
    echo "Server is healthy and ready for MCP clients."
    echo ""
    echo "Add to Claude Desktop (claude_desktop_config.json):"
    echo "  { \"mcpServers\": { \"flipper\": { \"url\": \"$BASE/mcp\" } } }"
else
    echo "Some checks failed. See docs/TROUBLESHOOTING.md for help."
    exit 1
fi
