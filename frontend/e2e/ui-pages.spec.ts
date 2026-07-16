import { test, type Page } from '@playwright/test';
import {
  expectNoTextOverlaps,
  expectNoHorizontalOverflow,
  expectViewportIsPhone,
} from '@xinutec/ui-harness';

/**
 * Phone-width layout checks for fleetwatch (Pixel 7, 412px). Every screen: no text
 * collisions, nothing spilling past the viewport. The overview screen is the
 * one that shipped a 24px-too-wide page (missing box-sizing reset); this locks
 * that fixed.
 *
 * fleetwatch fetches via httpResource; the backend is mocked with busy fixtures so
 * tiles, pills and the problems list all render at their fullest.
 */

const now = new Date('2026-07-03T20:00:00Z');
const ago = (s: number): string => new Date(now.getTime() - s * 1000).toISOString();
const inHours = (h: number): string => new Date(now.getTime() + h * 3600 * 1000).toISOString();

const OVERVIEW = [
  {
    source: 'mac-mini', collector: 'doc-checks', report_id: 'r1', collected_at: ago(780),
    age_s: 780, interval_s: 3600, freshness: 'fresh',
    worst: 'pass', pass: 82, warn: 0, fail: 0, skip: 0, total: 82,
  },
  {
    source: 'mac-mini', collector: 'fleet-health', report_id: 'r2', collected_at: ago(660),
    age_s: 660, interval_s: 3600, freshness: 'fresh',
    worst: 'warn', pass: 85, warn: 7, fail: 0, skip: 0, total: 92,
  },
  {
    source: 'mac-mini', collector: 'dependabot-and-container-image-freshness', report_id: 'r3',
    collected_at: ago(90000), age_s: 90000, interval_s: 86400, freshness: 'overdue',
    worst: 'fail', pass: 3, warn: 1, fail: 2, skip: 4, total: 10,
  },
];

const PROBLEMS = {
  checks: [
    {
      source: 'mac-mini', collector: 'fleet-health', report_id: 'r2', section: 'disk',
      label: 'root filesystem usage above threshold', subject: '/dev/disk1s1',
      verdict: 'warn', observed: '86%', expected: '< 80%', ref: null, collected_at: ago(660),
    },
    {
      source: 'mac-mini', collector: 'dependabot-and-container-image-freshness', report_id: 'r3',
      section: 'images', label: 'container image is behind the upstream tag',
      subject: 'xinutec/fleetwatch', verdict: 'fail', observed: 'sha 9dc8fec', expected: 'sha f335fb2',
      ref: null, collected_at: ago(90000),
    },
    // Hostile content (see the REPORT note): a long unbreakable path in observed.
    // Synthetic path — no real home dir / username.
    {
      source: 'mac-mini', collector: 'code-health', report_id: 'r4',
      section: 'lint', label: 'example-repo', subject: null, verdict: 'fail',
      observed:
        '/build/example/frontend/src/app/features/controllers/controller-list.component.scss:42',
      expected: null, ref: null, collected_at: ago(660),
    },
  ],
  muted: [
    {
      source: 'amun', collector: 'vpn-nodes', report_id: 'r5', section: 'wireguard',
      label: 'bes', subject: null, verdict: 'fail',
      observed: 'stale: last handshake 1d16h ago', ref: null, collected_at: ago(300),
      mute_id: '01J0MUTE0000000000000BES0', reason: 'Pi powered off — enough thermometer data from other devices',
      expires_at: inHours(20),
    },
  ],
  stale: [OVERVIEW[2]],
};

// A realistic trend: enough points for a line, mixed verdicts for the strip,
// and a null-value run (the chart must skip it, the timeline must not).
const HISTORY = {
  source: 'mac-mini', collector: 'fleet-health', section: 'disk',
  label: 'root filesystem usage above threshold', unit: '%',
  points: [
    ...Array.from({ length: 10 }, (_, i) => ({
      collected_at: ago((12 - i) * 21600),
      verdict: i === 7 ? 'warn' : 'pass',
      value: 43 + i * 4,
    })),
    { collected_at: ago(2 * 21600), verdict: 'skip', value: null },
    { collected_at: ago(21600), verdict: 'fail', value: 86 },
  ],
};

const REPORT = {
  id: 'r2', source: 'mac-mini', collector: 'fleet-health', schema: 1,
  collected_at: ago(660), received_at: ago(659), duration_ms: 4210, interval_s: 3600, ok: false,
  checks: [
    {
      section: 'disk', label: 'root filesystem usage above threshold', subject: '/dev/disk1s1',
      verdict: 'warn', observed: '86%', expected: '< 80%', value: 86, unit: '%',
      ref: null, detail: 'clean the nix store or the Backup volume',
    },
    {
      section: 'memory', label: 'swap in use', subject: null, verdict: 'pass',
      observed: '0 MB', expected: '0 MB', value: 0, unit: 'MB', ref: null, detail: null,
    },
    // Hostile content: long UNBREAKABLE tokens (a deep file path, a store hash)
    // with no spaces — the exact SHAPE that overflowed the phone in production.
    // Normal word-wrap can't break these, so without an overflow-wrap rule they
    // spill past the viewport. Keep this so the overflow assertion exercises the
    // worst case, not just tidy data. Paths are DELIBERATELY SYNTHETIC — never
    // put a real home dir / username / machine path in a committed fixture.
    {
      section: 'lint', label: 'example-repo',
      subject: '/build/example/frontend/src/app/features/controllers/controller-list.component.scss',
      verdict: 'fail',
      observed:
        "error (ignored): SQLite database '/build/cache/eval-cache/00000000aaaaaaaabbbbbbbbccccccccdddddddd11112222333344445555.sqlite' is busy",
      expected: null, value: 5, unit: 'violations', ref: null,
      detail:
        '/build/example/frontend/src/app/features/controllers/controller-list.component.scss:42 DL-SCSS-ADHOC-FONT-SIZE',
    },
  ],
};

async function mockApi(page: Page): Promise<void> {
  await page.route('**/api/overview', (r) => r.fulfill({ json: OVERVIEW }));
  await page.route('**/api/problems', (r) => r.fulfill({ json: PROBLEMS }));
  await page.route('**/api/reports/*', (r) => r.fulfill({ json: REPORT }));
  await page.route('**/api/history*', (r) =>
    r.fulfill({ json: HISTORY }),
  );
}

test('the suite really runs at phone geometry', async ({ page }) => {
  await mockApi(page);
  await page.goto('/');
  await expectViewportIsPhone(page);
});

test('overview — tiles + pills: lays out cleanly @ phone width', async ({ page }, testInfo) => {
  await mockApi(page);
  await page.goto('/');
  await page.getByText('doc-checks').waitFor();
  await page.getByText('fleet-health').waitFor();
  await expectNoTextOverlaps(page, testInfo);
  await expectNoHorizontalOverflow(page, testInfo);
});

test('problems — checks + stale list: lays out cleanly @ phone width', async ({ page }, testInfo) => {
  await mockApi(page);
  await page.goto('/problems');
  await page.getByText('root filesystem usage', { exact: false }).first().waitFor();
  // Scope to the scrolling content: with the mute actions + a muted section this
  // page is taller than the phone screen, so its lower rows sit UNDER the opaque
  // fixed bottom nav at scroll 0. That is normal (you scroll to reach them), but a
  // whole-body scan reads nav-label-over-content as a collision — the occlusion
  // false positive the harness documents for overlays. `main` excludes the nav
  // while still catching any real overlap within the content.
  await expectNoTextOverlaps(page, testInfo, 'main');
  await expectNoHorizontalOverflow(page, testInfo);
});

test('problems — open mute form: lays out cleanly @ phone width', async ({ page }, testInfo) => {
  await mockApi(page);
  await page.goto('/problems');
  await page.getByRole('button', { name: 'Mute this check' }).first().click();
  await page.getByText('Why is this expected?').waitFor();
  await expectNoTextOverlaps(page, testInfo, 'main');
  await expectNoHorizontalOverflow(page, testInfo);
});

test('history — chart + runs: lays out cleanly @ phone width', async ({ page }, testInfo) => {
  await mockApi(page);
  await page.goto(
    '/history?source=mac-mini&collector=fleet-health&section=disk' +
      '&label=root+filesystem+usage+above+threshold',
  );
  await page.getByText('Verdict timeline').waitFor();
  await expectNoTextOverlaps(page, testInfo, 'main');
  await expectNoHorizontalOverflow(page, testInfo);
});

test('report — one collector detail: lays out cleanly @ phone width', async ({ page }, testInfo) => {
  await mockApi(page);
  await page.goto('/reports/r2');
  await page.getByText('fleet-health', { exact: false }).first().waitFor();
  await expectNoTextOverlaps(page, testInfo);
  await expectNoHorizontalOverflow(page, testInfo);
});
