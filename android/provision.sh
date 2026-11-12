#!/usr/bin/env bash
# Provision the fleetwatch app on the Pixel 9: grant it notifications and give it
# the read token. Once per install from scratch; a reinstall keeps both, since
# the app's data survives `pm install -r`. The app itself is built and installed
# by xinutec-infra's android plan (`plan-run android-pixel9 --apply`).
#
#   ./provision.sh [<ip[:port]>]
#
# Keyed on the device *model*, never the IP: a Pixel 5 is often adb-connected too.
set -euo pipefail

PKG=org.xinutec.fleetwatch

# The fleetwatch READ token (opens GET /api/problems and nothing else — see
# FLEETWATCH_READ_TOKENS in the k8s secret). Distinct from the producers' ingest tokens:
# this one only ever reads, so a lost phone leaks no write access and no report history.
TOKEN_SVC=fleetwatch-read-token
TOKEN_ACCT=fleetwatch

# The VPN address (stable) first, then the LAN reservation; an argument wins.
CANDIDATES=("${1:-}" "10.100.0.12:5555" "192.168.1.133:5555")

for EP in "${CANDIDATES[@]}"; do
  [ -z "$EP" ] && continue
  [[ "$EP" == *:* ]] || EP="$EP:5555"
  adb connect "$EP" 2>&1 | grep -qiE "connected|already" || continue
  MODEL="$(adb -s "$EP" shell getprop ro.product.model 2>/dev/null | tr -d '\r')"
  if [ "$MODEL" != "Pixel 9" ]; then
    echo "  skip $EP — reports model '$MODEL', not 'Pixel 9'." >&2
    continue
  fi
  if ! adb -s "$EP" shell pm path "$PKG" | grep -q '^package:'; then
    echo "$PKG is not installed on the Pixel 9: plan-run android-pixel9 --apply first." >&2
    exit 1
  fi
  # The background poller needs to be able to speak: grant notifications up front so a
  # fresh install alerts without waiting for someone to tap through the runtime prompt.
  adb -s "$EP" shell pm grant "$PKG" android.permission.POST_NOTIFICATIONS || true
  # The read token into the app's private files over run-as, piped straight from the
  # Keychain, so it never lands in argv, a log, or this repo. `tee`, not `sh -c 'cat >
  # …'`: a nested shell loses run-as's app-dir cwd.
  adb -s "$EP" shell run-as "$PKG" mkdir -p files || true
  if /usr/bin/security find-generic-password -a "$TOKEN_ACCT" -s "$TOKEN_SVC" -w 2>/dev/null \
      | adb -s "$EP" shell run-as "$PKG" tee files/read_token >/dev/null; then
    echo "  read token in place on Pixel 9 ($EP)."
    exit 0
  fi
  echo "  WARN: no read token in the Keychain — the problem poller will stay idle." >&2
  echo "  Add one with:  security add-generic-password -a $TOKEN_ACCT -s $TOKEN_SVC -w <token>" >&2
  exit 1
done

echo "Pixel 9 not reachable on :5555 (VPN or LAN). Re-enable wireless debugging or" >&2
echo "re-run 'adb tcpip 5555', then pass the ip:port as an argument." >&2
exit 1
