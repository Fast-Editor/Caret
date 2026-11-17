#!/usr/bin/env bash
# Exercise a Warren daemon end to end without Caret: health, wallet, create an instance, wait for it, run a command,
# write and list a file, print a terminal URL, delete the instance.
#
# Usage: bash scripts/warren-smoke.sh [--keep]
#   --keep    leave the instance running (prints its id) instead of deleting it
#
# Connection (defaults match Caret's dev setup: the SSH tunnel from ~/Library/LaunchAgents/dev.caret.warren-tunnel.plist):
#   WARREN_CONNECT   host:port the requests are sent to            (default 127.0.0.1:7337)
#   WARREN_BASE      Warren base domain used in the Host header    (default warren.localhost:7337)
#   WARREN_API_KEY   sk_live_... key; default: the `warren` secret in ~/.caret/secrets.json
set -euo pipefail

CONNECT="${WARREN_CONNECT:-127.0.0.1:7337}"
BASE="${WARREN_BASE:-warren.localhost:7337}"
KEEP=0; [ "${1:-}" = "--keep" ] && KEEP=1
KEY="${WARREN_API_KEY:-$(python3 -c "import json,os;print(json.load(open(os.path.expanduser('~/.caret/secrets.json'))).get('warren',''))" 2>/dev/null || true)}"
[ -n "$KEY" ] || { echo "no API key: set WARREN_API_KEY or store one in Caret (Settings → Sandboxes)"; exit 1; }

API="http://$CONNECT/v1"
api() { # method path [json-body]
  local m="$1" p="$2" body="${3:-}"
  if [ -n "$body" ]; then curl -sS -m 60 -X "$m" -H "Host: api.$BASE" -H "Authorization: Bearer $KEY" -H 'Content-Type: application/json' -d "$body" "$API$p"
  else curl -sS -m 60 -X "$m" -H "Host: api.$BASE" -H "Authorization: Bearer $KEY" "$API$p"; fi
}
inst() { # method id path [raw-body]   (instance host: files API)
  local m="$1" id="$2" p="$3" body="${4:-}"
  if [ -n "$body" ]; then curl -sS -m 60 -X "$m" -H "Host: $id.$BASE" -H "X-Warren-Key: $KEY" --data-binary "$body" "http://$CONNECT/v1$p"
  else curl -sS -m 60 -X "$m" -H "Host: $id.$BASE" -H "X-Warren-Key: $KEY" "http://$CONNECT/v1$p"; fi
}
jget() { python3 -c "import json,sys; print(json.load(sys.stdin)[sys.argv[1]])" "$1"; }
step() { printf '\n== %s\n' "$*"; }

step "health ($API via Host api.$BASE)"
if ! curl -sS -m 5 -H "Host: api.$BASE" "$API/health"; then
  echo; echo "Warren is not reachable at $CONNECT. Is the tunnel up? (launchctl print gui/\$(id -u)/dev.caret.warren-tunnel; log: ~/Library/Logs/warren-tunnel.log)"; exit 1
fi; echo

step "wallet"
api GET /billing | python3 -c "import json,sys; d=json.load(sys.stdin); print(f\"wallet \${d['wallet_micros']/1e6:.2f}  awake {d['awake_count']}/{d['awake_limit']}\")"

step "create instance"
ID=$(api POST /instances '{"name":"smoke","auto_sleep":true}' | jget id)
echo "id: $ID"
for i in $(seq 1 60); do
  S=$(api GET "/instances/$ID" | jget status)
  [ "$S" = running ] && break
  [ "$S" = failed ] && { echo "instance failed"; exit 1; }
  sleep 1
done
echo "status: $S (after ${i}s)"

step "exec"
api POST "/instances/$ID/exec" '{"command":"uname -a && whoami && pwd && node --version && git --version"}' \
  | python3 -c "import json,sys; d=json.load(sys.stdin); print(d['stdout'].rstrip()); print(f\"exit {d['exit_code']} in {d['duration_ms']} ms\")"

step "files (instance host $ID.$BASE)"
inst PUT "$ID" "/files/content?path=hello.txt" "hello from $(hostname -s) $(date +%H:%M:%S)"; echo
inst GET "$ID" "/files?path=."; echo

step "terminal URL (ttyd, valid 10 min)"
api GET "/instances/$ID/signed-url?port=7681&ttl=600" | jget url

if [ "$KEEP" = 1 ]; then
  step "kept instance $ID (delete with: curl -X DELETE -H 'Host: api.$BASE' -H 'Authorization: Bearer \$KEY' $API/instances/$ID)"
else
  step "delete"
  api DELETE "/instances/$ID"; echo
fi
step "done"
