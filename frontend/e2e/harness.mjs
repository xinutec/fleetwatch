// The app-specific half of the shared phone-width harness (@xinutec/ui-harness).
// Read by BOTH playwright.config.ts and the harness's static server, so there is
// one place to say what this app is and no port to keep in step — the port is
// allocated from `app`.

/** @type {import('@xinutec/ui-harness/config').HarnessSpec} */
export default {
  app: 'fleetwatch',
  dist: 'dist/fleetwatch-web/browser',
  // No API stub. The one that used to sit in serve.mjs was life's, verbatim
  // ('/api/items' with a cached avocado) — an endpoint this app does not have,
  // arriving with the file it was copied from. The specs page.route everything,
  // and anything they leave unrouted answers `[]`.
};
