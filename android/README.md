# fleetwatch web viewer (Android)

The `fleetwatch.xinutec.org` fleet monitoring dashboard presented as a native-feeling
app: a full-screen **WebView**, no address bar, no tabs, a home-screen icon — plus a
**background poller** that tells you when something breaks, so you don't have to
remember to look.

The site is **private** (reachable only over the VPN); reads are behind a Nextcloud
login, which the WebView performs interactively.

## What it does

- Loads `https://fleetwatch.xinutec.org/` — **hardcoded** (`MainActivity.FLEETWATCH_URL`);
  this app is single-purpose.
- JavaScript + DOM storage on (Angular), all navigation kept in-app, Back walks
  the SPA history; reopens on the last in-app page — but never on a login hop
  (`Restore.isRestorable`). An OAuth callback carries a one-shot code, so saving
  one as the restore point turns a single failed login into a permanent one: the
  app relaunches into a spent callback, is refused, and never reaches the
  dashboard to try again. That is exactly how it stranded itself on 2026-07-28.
- Insets the WebView from the system bars by padding a wrapper, and paints the
  strips behind the bars with the page's own surface colour (read on load, so it
  tracks the Material light/dark theme).
- **Polls `GET /api/problems` every 30 minutes** (`ProblemsWorker`) and raises a
  notification **when the set of failures changes** (warnings are dashboard-only — see
  below). Tapping it opens `/problems`.

Runs on any Android 8+ (minSdk 26) device. Must be on the VPN to reach the host.

## The poller

fleetwatch is a pull-based dashboard — the server never reaches out. That makes a
problem exactly as visible as someone's willingness to open the page, and that gap is
not hypothetical: the `pixel5` sensor receiver went deaf and stayed silent for **7
hours**, caught only because a human noticed a line missing from a chart. So the phone
asks on a timer; the server stays passive.

- **Cadence:** 30 min, via WorkManager (above its 15-minute floor), network-constrained.
  WorkManager batches the wakeup with the system's, so the battery cost is ~0: one HTTPS
  GET per run.
- **Failures only, never warnings** (`Problems.notifiable()`): the notification is
  reserved for what is broken — a failing check or a silent producer. A warning is
  something to know, not something to do, and some are true indefinitely by design (the
  fleet check warns, permanently, that amun is *deliberately* held a NixOS release
  behind). Pushing those to a phone reports a standing decision as news, and teaches you
  to ignore the channel that carries the real failures. Warnings stay on the dashboard,
  where you go to look. Escalation still fires: the moment the same check turns from warn
  to fail, it notifies.
- **Only on change:** the problem set is fingerprinted (`Problems.fingerprint()`) and a
  notification fires only when it differs from the last poll. Re-notifying every 30 min
  about a problem you already know about trains you to swipe alerts away unread — at
  which point the alerting is worse than none, because it looks like it works. The
  fingerprint deliberately ignores `observed`, which changes every poll during an
  ongoing outage ("last push 20 min ago" → "50 min ago").
- **Recovery clears it:** an empty problem set cancels the standing notification.
- **Auth — a read token, not the session cookie.** A WorkManager job can't complete an
  interactive Nextcloud login, and reusing the WebView's NC cookie would work until it
  quietly expired, leaving a monitor that silently stops monitoring. So the poller uses
  a bearer **read token** (`FLEETWATCH_READ_TOKENS` server-side) which opens
  `/api/problems` and *nothing else* — a lost phone leaks no writes and no history.
- **The token** lives in a private file (`filesDir/read_token`), injected by `deploy.sh`
  from the Mac Keychain (`fleetwatch-read-token`) over `adb run-as`. It never appears in
  argv, a log, or this repo. Without it the poller stays idle (and says so in logcat)
  rather than nagging a fresh install about its own setup.

## Build & install

No toolchain lives in this repo — it borrows the recall project's `android` nix
dev shell (JDK 17 + Android SDK; the Gradle wrapper pins Gradle):

```sh
cd android
nix develop ~/Code/recall#android --command ./gradlew :app:assembleDebug
# → app/build/outputs/apk/debug/app-debug.apk
```

Or build + install to the Pixel 9 in one step (keys on device model, not IP):

```sh
nix develop ~/Code/recall#android --command ./deploy.sh
```

The APK is signed with the auto-generated debug key — fine for sideloading, the
only distribution path.

## Layout

```
android/
├── app/
│   ├── build.gradle.kts                          # android app module, no Compose/AppCompat
│   └── src/main/
│       ├── AndroidManifest.xml                   # INTERNET + POST_NOTIFICATIONS
│       ├── kotlin/org/xinutec/fleetwatch/
│       │   ├── MainActivity.kt                   # the WebView (+ inset padding)
│       │   ├── Restore.kt                        # which pages may be the reopen-on page
│       │   ├── Problems.kt                       # /api/problems: parse, fingerprint, summarise
│       │   └── Watch.kt                          # ProblemsWorker (30-min poll) + token storage
│       └── res/                                  # launcher icon (heartbeat), theme, strings
├── build.gradle.kts · settings.gradle.kts · gradle/   # project scaffolding
└── gradlew                                       # borrows ~/Code/recall#android for the SDK
```
