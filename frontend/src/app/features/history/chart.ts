// Pure chart math for the history view — scaling, degenerate inputs, and
// layout live here, free of Angular, so they're unit-testable (chart.spec.ts).
// The SVG stays hand-rolled: at one line + dots + a verdict strip, a chart
// library would be more code than this file.

import { HistoryPoint, Verdict } from '../../models';

/** SVG viewBox of the value chart (CSS scales it responsively). */
export const W = 320;
export const H = 90;
export const PAD = 8;

export interface Dot {
  x: number;
  y: number;
  verdict: Verdict;
  value: number;
  at: string;
}

export interface Tick {
  x: number;
  verdict: Verdict;
  at: string;
}

export interface GridLine {
  y: number;
  value: number;
}

export interface Chart {
  path: string;
  dots: Dot[];
  min: number;
  max: number;
  /** Horizontal reference lines at max / mid / min. */
  grid: GridLine[];
  /** Time extent of the plotted points (epoch ms) — the x-axis labels. */
  fromMs: number;
  toMs: number;
}

/** `[t0, t1]` (epoch ms) of the given instants; a degenerate span is widened
 *  by 1ms so x-scaling never divides by zero (a single-instant history still
 *  lands inside the chart instead of at NaN). */
export function timeSpan(times: string[]): [number, number] {
  const ms = times.map((t) => Date.parse(t));
  const t0 = Math.min(...ms);
  const t1 = Math.max(...ms);
  return [t0, t1 === t0 ? t0 + 1 : t1];
}

/** Map an instant into the padded [PAD, W-PAD] x-range. */
export function xOf(t: number, t0: number, t1: number): number {
  return PAD + ((t - t0) / (t1 - t0)) * (W - 2 * PAD);
}

/** Time-ordered verdict ticks for the full timeline strip (numeric or not). */
export function buildTicks(points: HistoryPoint[]): Tick[] {
  if (points.length === 0) return [];
  const [t0, t1] = timeSpan(points.map((p) => p.collected_at));
  return points.map((p) => ({
    x: xOf(Date.parse(p.collected_at), t0, t1),
    verdict: p.verdict,
    at: p.collected_at,
  }));
}

/** SVG line chart over the numeric points, or null if fewer than two. */
export function buildChart(points: HistoryPoint[]): Chart | null {
  const pts = points.flatMap((p) =>
    p.value === null ? [] : [{ at: p.collected_at, verdict: p.verdict, value: p.value }],
  );
  if (pts.length < 2) return null;

  const values = pts.map((p) => p.value);
  let min = Math.min(...values);
  let max = Math.max(...values);
  if (min === max) {
    // A flat series still deserves a line in the middle, not a div-by-zero.
    min -= 1;
    max += 1;
  }
  const [t0, t1] = timeSpan(pts.map((p) => p.at));
  const yOf = (v: number) => PAD + (1 - (v - min) / (max - min)) * (H - 2 * PAD);

  const dots: Dot[] = pts.map((p) => ({
    x: xOf(Date.parse(p.at), t0, t1),
    y: yOf(p.value),
    verdict: p.verdict,
    value: p.value,
    at: p.at,
  }));
  const path = dots
    .map((d, i) => `${i === 0 ? 'M' : 'L'}${d.x.toFixed(1)},${d.y.toFixed(1)}`)
    .join(' ');
  const mid = (min + max) / 2;
  const grid: GridLine[] = [max, mid, min].map((value) => ({ value, y: yOf(value) }));
  return { path, dots, min, max, grid, fromMs: t0, toMs: t1 };
}
