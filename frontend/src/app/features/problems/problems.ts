import {
  ChangeDetectionStrategy,
  Component,
  DestroyRef,
  computed,
  effect,
  inject,
  signal,
} from '@angular/core';
import { httpResource } from '@angular/common/http';
import { FormsModule } from '@angular/forms';
import { RouterLink } from '@angular/router';
import { MatButtonModule } from '@angular/material/button';
import { MatCardModule } from '@angular/material/card';
import { MatFormFieldModule } from '@angular/material/form-field';
import { MatIconModule } from '@angular/material/icon';
import { MatInputModule } from '@angular/material/input';
import { MatProgressBarModule } from '@angular/material/progress-bar';
import { MatSelectModule } from '@angular/material/select';
import { MatSnackBar } from '@angular/material/snack-bar';

import { OverviewEntry, ProblemCheck, Problems as ProblemsData } from '../../models';
import { formatAge } from '../../status';
import { MUTE_DURATIONS, MutesApi, expiresIn } from '../../mutes';

const EMPTY: ProblemsData = { checks: [], muted: [], stale: [] };

/** How often the open page re-asks the server (only while visible). */
export const REFRESH_MS = 90_000;

/** Identity of the check a mute targets — its source/collector/label. */
function keyOf(c: ProblemCheck): string {
  return `${c.source} ${c.collector} ${c.label}`;
}

@Component({
  selector: 'app-problems',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [
    RouterLink,
    FormsModule,
    MatButtonModule,
    MatCardModule,
    MatFormFieldModule,
    MatIconModule,
    MatInputModule,
    MatProgressBarModule,
    MatSelectModule,
  ],
  templateUrl: './problems.html',
  styleUrl: './problems.scss',
})
export class Problems {
  private readonly mutes = inject(MutesApi);
  private readonly snack = inject(MatSnackBar);

  readonly data = httpResource<ProblemsData>(() => '/api/problems', { defaultValue: EMPTY });
  readonly formatAge = formatAge;
  readonly expiresIn = expiresIn;
  readonly durations = MUTE_DURATIONS;

  // Which problem row has its mute form open (its identity key), plus the form's
  // fields. A reason is mandatory — the backend rejects an empty one.
  readonly openKey = signal<string | null>(null);
  readonly reason = signal('');
  readonly ttlHours = signal(MUTE_DURATIONS[2].hours); // default: 1 day
  readonly saving = signal(false);

  readonly keyOf = keyOf;

  readonly nothingWrong = computed(() => {
    const d = this.data.value();
    return (
      !this.data.isLoading() &&
      d.checks.length === 0 &&
      d.muted.length === 0 &&
      d.stale.length === 0
    );
  });

  // Green must mean "verified green", not "no data": when everything is clear,
  // fetch the overview once and say how many collectors that verdict covers and
  // how fresh the newest report is.
  readonly overview = httpResource<OverviewEntry[]>(() =>
    this.nothingWrong() ? '/api/overview' : undefined,
  );
  readonly allClearDetail = computed<string | null>(() => {
    const entries = this.overview.value();
    if (!entries || entries.length === 0) return null;
    const newest = Math.min(...entries.map((e) => e.age_s));
    const n = entries.length;
    return `${n} collector${n === 1 ? '' : 's'} reporting · newest report ${formatAge(newest)}`;
  });

  // "checked 3m ago" in the header — the honesty marker for a page that sits
  // open. Stamped when a load lands; aged by a coarse ticker.
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

    // A status page left open must not quietly go stale: re-ask on a gentle
    // cadence while the tab is visible, and immediately when the tab becomes
    // visible again after more than one cadence in the background. Hidden tabs
    // do nothing — no wasted requests from a phone in a pocket.
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

  toggleMute(c: ProblemCheck): void {
    const key = keyOf(c);
    if (this.openKey() === key) {
      this.openKey.set(null);
      return;
    }
    this.reason.set('');
    this.ttlHours.set(MUTE_DURATIONS[2].hours);
    this.openKey.set(key);
  }

  submitMute(c: ProblemCheck): void {
    const reason = this.reason().trim();
    if (!reason || this.saving()) return;
    this.saving.set(true);
    this.mutes
      .create({
        source: c.source,
        collector: c.collector,
        label: c.label,
        reason,
        ttl_hours: this.ttlHours(),
      })
      .subscribe({
        next: () => {
          this.saving.set(false);
          this.openKey.set(null);
          this.data.reload();
        },
        error: () => {
          // The form stays open with the typed reason — failing silently would
          // read as "muted" while the alert keeps firing.
          this.saving.set(false);
          this.snack.open('Could not save the mute', 'Dismiss', { duration: 5000 });
        },
      });
  }

  unmute(id: string): void {
    this.mutes.remove(id).subscribe({
      next: () => this.data.reload(),
      error: () => this.snack.open('Could not remove the mute', 'Dismiss', { duration: 5000 }),
    });
  }
}
