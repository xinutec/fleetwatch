// Mute mutations (create / unmute). Reads come through httpResource in the
// views; these are the two write calls, kept in one small injectable so the
// components stay declarative.

import { Injectable, inject } from '@angular/core';
import { HttpClient } from '@angular/common/http';

import { Mute, NewMute } from './models';

@Injectable({ providedIn: 'root' })
export class MutesApi {
  private readonly http = inject(HttpClient);

  create(mute: NewMute) {
    return this.http.post<Mute>('/api/mutes', mute);
  }

  remove(id: string) {
    return this.http.delete<void>(`/api/mutes/${id}`);
  }
}

/** The mute durations offered in the UI, shortest first. A mute always expires
 *  (the backend clamps to ≤90d) — these are the sensible presets. */
export const MUTE_DURATIONS: readonly { label: string; hours: number }[] = [
  { label: '1 hour', hours: 1 },
  { label: '8 hours', hours: 8 },
  { label: '1 day', hours: 24 },
  { label: '3 days', hours: 72 },
  { label: '1 week', hours: 168 },
];

/** Human "expires in …" from an ISO instant, e.g. "in 3h", "in 2d". Past → "expired". */
export function expiresIn(iso: string): string {
  const secs = Math.round((new Date(iso).getTime() - Date.now()) / 1000);
  if (secs <= 0) return 'expired';
  if (secs < 3600) return `in ${Math.max(1, Math.round(secs / 60))}m`;
  if (secs < 86400) return `in ${Math.round(secs / 3600)}h`;
  return `in ${Math.round(secs / 86400)}d`;
}
