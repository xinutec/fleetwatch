import { describe, expect, it } from 'vitest';
import { expiresIn } from './mutes';

describe('expiresIn', () => {
  const inSeconds = (s: number) => new Date(Date.now() + s * 1000).toISOString();

  it('renders a compact future distance', () => {
    expect(expiresIn(inSeconds(90 * 60))).toBe('in 2h'); // rounds
    expect(expiresIn(inSeconds(5 * 60))).toBe('in 5m');
    expect(expiresIn(inSeconds(2 * 86400))).toBe('in 2d');
  });

  it('never shows "in 0m" — a sub-minute mute still rounds up to a minute', () => {
    expect(expiresIn(inSeconds(20))).toBe('in 1m');
  });

  it('a lapsed instant reads as expired, not a negative distance', () => {
    expect(expiresIn(inSeconds(-60))).toBe('expired');
  });
});
