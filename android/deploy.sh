#!/usr/bin/env bash
# Build the fleetwatch dashboard APK and install it to the Pixel 9 over Wi-Fi. Run from
# android/ inside the borrowed Android dev shell:
#
#   nix develop ~/Code/recall#android --command ./deploy.sh [<ip[:port]>]
#
# This is a single-purpose handheld app on ONE phone (the Pixel 9). DHCP drifts the
# IP, so we key on the device *model*, never the IP: connect, verify it really is a
# Pixel 9, then install by serial. A bare `adb install` could hit the wrong device
# (a Pixel 5 is often also adb-connected) — so we never use it.
set -euo pipefail
cd "$(dirname "$0")"

ADB="$ANDROID_HOME/platform-tools/adb"
PKG=org.xinutec.fleetwatch

# The fleetwatch READ token (opens GET /api/problems and nothing else — see
# FLEETWATCH_READ_TOKENS in the k8s secret). Distinct from the producers' ingest tokens:
# this one only ever reads, so a lost phone leaks no write access and no report history.
TOKEN_SVC=fleetwatch-read-token
TOKEN_ACCT=fleetwatch

echo "building APK…"
./gradlew :app:assembleDebug -q
APK="$PWD/app/build/outputs/apk/debug/app-debug.apk"

# Endpoints to try, in order. :5555 (persistent `adb tcpip`) survives sleep, so try
# it first — VPN IP (stable, 10.100.0.12) then the LAN DHCP reservation. Override
# with an arg if wireless debugging rotated to a random port.
CANDIDATES=("${1:-}" "10.100.0.12:5555" "192.168.1.133:5555")

for EP in "${CANDIDATES[@]}"; do
  [ -z "$EP" ] && continue
  [[ "$EP" == *:* ]] || EP="$EP:5555"
  "$ADB" connect "$EP" 2>&1 | grep -qiE "connected|already" || continue
  MODEL="$("$ADB" -s "$EP" shell getprop ro.product.model 2>/dev/null | tr -d '\r')"
  if [ "$MODEL" != "Pixel 9" ]; then
    echo "  skip $EP — reports model '$MODEL', not 'Pixel 9'." >&2
    continue
  fi
  echo "=== installing to Pixel 9 ($EP) ==="
  "$ADB" -s "$EP" install -r "$APK"
  # The background poller needs to be able to speak: grant notifications up front so a
  # fresh install alerts without waiting for someone to tap through the runtime prompt.
  "$ADB" -s "$EP" shell pm grant "$PKG" android.permission.POST_NOTIFICATIONS || true
  # Inject the read token (GET /api/problems only) into the app's private files dir over
  # run-as. Piped straight from the Keychain, so it never lands in argv, a log, or this
  # repo. `tee`, not `sh -c 'cat > …'`: a nested shell loses run-as's app-dir cwd.
  "$ADB" -s "$EP" shell run-as "$PKG" mkdir -p files || true
  if /usr/bin/security find-generic-password -a "$TOKEN_ACCT" -s "$TOKEN_SVC" -w 2>/dev/null \
      | "$ADB" -s "$EP" shell run-as "$PKG" tee files/read_token >/dev/null; then
    echo "  read token injected."
  else
    echo "  WARN: no read token set — the problem poller will stay idle." >&2
    echo "  Add one with:  security add-generic-password -a $TOKEN_ACCT -s $TOKEN_SVC -w <token>" >&2
  fi
  "$ADB" -s "$EP" shell am start -S -n org.xinutec.fleetwatch/.MainActivity >/dev/null
  echo "  installed + launched on Pixel 9 ($EP)."
  exit 0
done

echo "Pixel 9 not reachable on :5555 (VPN or LAN). Re-enable wireless debugging or" >&2
echo "re-run 'adb tcpip 5555', then pass the ip:port as an argument." >&2
exit 1
