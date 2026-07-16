import { describe, expect, it } from 'vitest';
import { HistoryPoint } from '../../models';
import { H, PAD, W, buildChart, buildTicks, timeSpan } from './chart';

function pt(at: string, value: number | null, verdict: HistoryPoint['verdict'] = 'pass'): HistoryPoint {
  return { collected_at: at, verdict, value };
}

describe('buildChart', () => {
  it('needs two numeric points — non-numeric ones do not count', () => {
    expect(buildChart([])).toBeNull();
    expect(buildChart([pt('2026-07-01T00:00:00Z', 5)])).toBeNull();
    expect(
      buildChart([pt('2026-07-01T00:00:00Z', 5), pt('2026-07-02T00:00:00Z', null)]),
    ).toBeNull();
  });

  it('scales values into the padded box: min at the bottom, max at the top', () => {
    const ch = buildChart([pt('2026-07-01T00:00:00Z', 0), pt('2026-07-02T00:00:00Z', 100)])!;
    expect(ch.dots[0].x).toBe(PAD);
    expect(ch.dots[0].y).toBe(H - PAD); // min → bottom
    expect(ch.dots[1].x).toBe(W - PAD);
    expect(ch.dots[1].y).toBe(PAD); // max → top
    expect(ch.min).toBe(0);
    expect(ch.max).toBe(100);
  });

  it('a flat series is widened, not divided by zero', () => {
    const ch = buildChart([pt('2026-07-01T00:00:00Z', 5), pt('2026-07-02T00:00:00Z', 5)])!;
    expect(ch.min).toBe(4);
    expect(ch.max).toBe(6);
    // Both dots sit on the midline, well inside the box.
    for (const d of ch.dots) {
      expect(d.y).toBe(H / 2);
    }
  });

  it('two runs at the same instant still produce finite coordinates', () => {
    const ch = buildChart([pt('2026-07-01T00:00:00Z', 1), pt('2026-07-01T00:00:00Z', 2)])!;
    for (const d of ch.dots) {
      expect(Number.isFinite(d.x)).toBe(true);
      expect(Number.isFinite(d.y)).toBe(true);
    }
  });

  it('non-numeric points are skipped, keeping the line over the numeric ones', () => {
    const ch = buildChart([
      pt('2026-07-01T00:00:00Z', 10),
      pt('2026-07-02T00:00:00Z', null, 'skip'),
      pt('2026-07-03T00:00:00Z', 20),
    ])!;
    expect(ch.dots).toHaveLength(2);
    expect(ch.dots.map((d) => d.value)).toEqual([10, 20]);
  });

  it('the path visits every dot in time order', () => {
    const ch = buildChart([
      pt('2026-07-01T00:00:00Z', 1),
      pt('2026-07-02T00:00:00Z', 3),
      pt('2026-07-03T00:00:00Z', 2),
    ])!;
    expect(ch.path).toMatch(/^M[\d.]+,[\d.]+ L[\d.]+,[\d.]+ L[\d.]+,[\d.]+$/);
  });

  it('grid lines sit at max, mid, and min', () => {
    const ch = buildChart([pt('2026-07-01T00:00:00Z', 0), pt('2026-07-02T00:00:00Z', 100)])!;
    expect(ch.grid.map((g) => g.value)).toEqual([100, 50, 0]);
    expect(ch.grid.map((g) => g.y)).toEqual([PAD, H / 2, H - PAD]);
  });

  it('the x-axis extent is the numeric points’ time span', () => {
    const ch = buildChart([
      pt('2026-07-01T00:00:00Z', null),
      pt('2026-07-02T00:00:00Z', 1),
      pt('2026-07-03T00:00:00Z', 2),
    ])!;
    expect(ch.fromMs).toBe(Date.parse('2026-07-02T00:00:00Z'));
    expect(ch.toMs).toBe(Date.parse('2026-07-03T00:00:00Z'));
  });
});

describe('buildTicks', () => {
  it('is empty for no points', () => {
    expect(buildTicks([])).toEqual([]);
  });

  it('includes non-numeric points — the verdict strip covers every run', () => {
    const ticks = buildTicks([
      pt('2026-07-01T00:00:00Z', 1, 'pass'),
      pt('2026-07-02T00:00:00Z', null, 'fail'),
    ]);
    expect(ticks).toHaveLength(2);
    expect(ticks[0].x).toBe(PAD);
    expect(ticks[1].x).toBe(W - PAD);
    expect(ticks[1].verdict).toBe('fail');
  });
});

describe('timeSpan', () => {
  it('widens a single instant so scaling never divides by zero', () => {
    const t = '2026-07-01T00:00:00Z';
    const [t0, t1] = timeSpan([t, t]);
    expect(t1).toBeGreaterThan(t0);
  });
});
