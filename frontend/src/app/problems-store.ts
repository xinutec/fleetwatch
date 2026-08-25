// The one live view of /api/problems, shared by the nav badge and the
// problems page. Before this existed each had its own httpResource: two
// identical fetches at startup, and a badge that never refreshed — a tab left
// open showed a live page next to a fossilised badge.

import { DestroyRef, Injectable, computed, effect, inject, signal } from '@angular/core';
import { httpResource } from '@angular/common/http';

import { Problems } from './models';
import { formatAge } from './status';

// `lapsing` is deliberately NOT in `count` below: a mute about to expire is a
// decision to make, not a fault. Putting it in the badge would light the app up
// for something that is not wrong yet.
const EMPTY: Problems = { checks: [], muted: [], stale: [], lapsing: [], returned: [] };

/** How often the open tab re-asks the server (only while visible). */
export const REFRESH_MS = 90_000;

@Injectable({ providedIn: 'root' })
export class ProblemsStore {
  readonly data = httpResource<Problems>(() => '/api/problems', { defaultValue: EMPTY });

  /** Standing badge count: failing/warning checks + overdue/silent collectors. */
  readonly count = computed(
    () =>
      this.data.value().checks.length +
      this.data.value().stale.length +
      this.data.value().returned.length,
  );

  // "checked 3m ago" — the honesty marker for a page that sits open. Stamped
  // when a load lands; aged by a coarse ticker.
  private readonly fetchedAt = signal<number | null>(null);
  private readonly nowMs = signal(Date.now());
  readonly checkedAgo = computed<string | null>(() => {
    const at = this.fetchedAt();
    if (at === null) return null;
    return formatAge(Math.max(0, Math.round((this.nowMs() - at) / 1000)));
  });

  constructor() {
    effect(() => {
      if (this.data.status() === 'resolved') this.fetchedAt.set(Date.now());
    });

    // A status app left open must not quietly go stale: re-ask on a gentle
    // cadence while the tab is visible, and immediately when the tab becomes
    // visible again after more than one cadence in the background. Hidden tabs
    // do nothing — no wasted requests from a phone in a pocket. App-lifetime
    // by design (root service); DestroyRef still cleans up under TestBed.
    const refresh = setInterval(() => {
      if (!document.hidden) this.data.reload();
    }, REFRESH_MS);
    const tick = setInterval(() => this.nowMs.set(Date.now()), 10_000);
    const onVisible = () => {
      const at = this.fetchedAt();
      if (!document.hidden && at !== null && Date.now() - at > REFRESH_MS) {
        this.data.reload();
      }
    };
    document.addEventListener('visibilitychange', onVisible);
    inject(DestroyRef).onDestroy(() => {
      clearInterval(refresh);
      clearInterval(tick);
      document.removeEventListener('visibilitychange', onVisible);
    });
  }
}
