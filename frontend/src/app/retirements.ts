// Retirement mutations (retire / un-retire). Reads come through the problems
// view; these are the two write calls, kept beside `mutes.ts` because they are
// the same shape of thing — a deliberate, attributed decision to stop being
// told about something.
//
// The difference is the whole design: a mute expires and a retirement does not,
// so there is no duration to choose here. See migrations/0007_retirement.sql.

import { Injectable, inject } from '@angular/core';
import { HttpClient } from '@angular/common/http';

import { NewRetirement, Retirement } from './models';

@Injectable({ providedIn: 'root' })
export class RetirementsApi {
  private readonly http = inject(HttpClient);

  create(r: NewRetirement) {
    return this.http.post<Retirement>('/api/retirements', r);
  }

  remove(source: string, collector: string) {
    return this.http.delete<void>(
      `/api/retirements/${encodeURIComponent(source)}/${encodeURIComponent(collector)}`,
    );
  }
}
