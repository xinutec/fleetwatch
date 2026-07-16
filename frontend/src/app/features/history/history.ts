import { ChangeDetectionStrategy, Component, computed, input, signal } from '@angular/core';
import { httpResource } from '@angular/common/http';
import { DatePipe } from '@angular/common';
import { MatCardModule } from '@angular/material/card';
import { MatIconModule } from '@angular/material/icon';
import { MatProgressBarModule } from '@angular/material/progress-bar';

import { History as HistoryData } from '../../models';
import { fmtValue } from '../../status';
import { Chart, Dot, H, Tick, W, buildChart, buildTicks } from './chart';

@Component({
  selector: 'app-history',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [DatePipe, MatCardModule, MatIconModule, MatProgressBarModule],
  templateUrl: './history.html',
  styleUrl: './history.scss',
})
export class History {
  // Bound from query params (withComponentInputBinding). The router passes
  // undefined for an absent param (overriding the default) — normalize it,
  // so the declared string type is honest.
  readonly source = input('', { transform: (v: string | undefined) => v ?? '' });
  readonly collector = input('', { transform: (v: string | undefined) => v ?? '' });
  readonly section = input('', { transform: (v: string | undefined) => v ?? '' });
  readonly label = input('', { transform: (v: string | undefined) => v ?? '' });

  // Idle (no request) until all four params are present; otherwise fetches and
  // re-fetches whenever any of them changes.
  readonly data = httpResource<HistoryData>(() => {
    const s = this.source();
    const c = this.collector();
    const sec = this.section();
    const l = this.label();
    if (!s || !c || !sec || !l) return undefined;
    const p = new URLSearchParams({ source: s, collector: c, section: sec, label: l });
    return `/api/history?${p.toString()}`;
  });

  readonly w = W;
  readonly h = H;
  readonly fmt = fmtValue;

  readonly ticks = computed<Tick[]>(() => buildTicks(this.data.value()?.points ?? []));
  readonly chart = computed<Chart | null>(() => buildChart(this.data.value()?.points ?? []));

  readonly latest = computed(() => {
    const pts = this.data.value()?.points ?? [];
    return pts.length ? pts[pts.length - 1] : null;
  });

  // The tapped dot. SVG <title> tooltips need a hover, which a phone doesn't
  // have — tapping a point shows its value + time in the caption row instead.
  // Derived against the current chart so a selection can't outlive its data
  // (navigating to another check resets it by construction).
  private readonly tapped = signal<Dot | null>(null);
  readonly selected = computed<Dot | null>(() => {
    const s = this.tapped();
    const ch = this.chart();
    return s && ch?.dots.some((d) => d.at === s.at && d.value === s.value) ? s : null;
  });

  select(d: Dot): void {
    this.tapped.set(this.selected()?.at === d.at ? null : d);
  }
}
