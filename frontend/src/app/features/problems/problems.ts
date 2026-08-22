import { ChangeDetectionStrategy, Component, computed, inject, signal } from '@angular/core';
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

import { OverviewEntry, ProblemCheck } from '../../models';
import { failingFor, formatAge } from '../../status';
import { MUTE_DURATIONS, MutesApi, expiresIn } from '../../mutes';
import { ProblemsStore } from '../../problems-store';

/** Identity of the check a mute targets — its source/collector/label.
 *  Structural (not concatenated), so a field containing the would-be
 *  separator can't make two rows share an open-form key. */
function keyOf(c: ProblemCheck): string {
  return JSON.stringify([c.source, c.collector, c.label]);
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
  private readonly store = inject(ProblemsStore);

  // The shared live problem set (also drives the nav badge) — reloading here
  // refreshes both.
  readonly data = this.store.data;
  readonly checkedAgo = this.store.checkedAgo;
  readonly formatAge = formatAge;
  readonly failingFor = failingFor;


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
      d.stale.length === 0 &&
      // A mute about to lapse is not a fault, but it IS something on this page.
      // Rendering a green tick and "all clear" directly above two mutes running
      // out reads as a contradiction — and worse, invites the glance that stops
      // at the tick, which is the glance this section exists to interrupt.
      d.lapsing.length === 0
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
