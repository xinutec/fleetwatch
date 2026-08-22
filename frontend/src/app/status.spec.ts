import { describe, expect, it } from 'vitest';
import { failingFor, fmtValue, formatAge, freshnessLabel, tileClass } from './status';

describe('status helpers', () => {
  it('tile shows worst verdict when fresh', () => {
    expect(tileClass('pass', 'fresh')).toBe('pass');
    expect(tileClass('warn', 'fresh')).toBe('warn');
    expect(tileClass('fail', 'fresh')).toBe('fail');
  });

  it('staleness overrides the verdict — a dead producer is never green', () => {
    expect(tileClass('pass', 'overdue')).toBe('warn');
    expect(tileClass('pass', 'silent')).toBe('fail');
    // even if the last data was passing, silence wins.
    expect(tileClass('pass', 'silent')).not.toBe('pass');
  });

  it('labels only non-fresh states', () => {
    expect(freshnessLabel('fresh')).toBeNull();
    expect(freshnessLabel('overdue')).toBe('overdue');
    expect(freshnessLabel('silent')).toBe('no data');
  });

  it('attaches symbol units, spaces word units', () => {
    expect(fmtValue(43, '%')).toBe('43%');
    expect(fmtValue(0, 'violations')).toBe('0 violations');
    expect(fmtValue(68, 'days')).toBe('68 days');
    expect(fmtValue(12, null)).toBe('12');
    expect(fmtValue(21, '')).toBe('21');
  });

  it('formats age in coarse human units', () => {
    expect(formatAge(10)).toBe('just now');
    expect(formatAge(120)).toBe('2m ago');
    expect(formatAge(3600)).toBe('1h ago');
    expect(formatAge(3 * 86400)).toBe('3d ago');
  });
});


describe('failingFor', () => {
  // ⚠ The one-hour threshold IS the feature, not a detail. A fault minutes old
  // says nothing a red row does not already say; labelling every row would bury
  // the case this exists for — the row red for EIGHT DAYS that looked identical
  // to the one that broke a minute ago (claude-disk, 2026-08-21).
  it('says nothing for a fault minutes old', () => {
    expect(failingFor(new Date(Date.now() - 10 * 60_000).toISOString())).toBeNull();
  });

  it('reports a standing fault in days', () => {
    expect(failingFor(new Date(Date.now() - 8 * 86_400_000).toISOString())).toBe('8d');
  });

  it('says nothing when the server does not know', () => {
    // A run older than the backfill. "failing just now" for an unknown start
    // would be an invention, which is worse than saying nothing.
    expect(failingFor(null)).toBeNull();
  });

  it('says nothing for an unparseable timestamp rather than NaN', () => {
    expect(failingFor('not a date')).toBeNull();
  });
});
