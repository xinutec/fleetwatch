import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { TestBed, ComponentFixture } from '@angular/core/testing';
import { provideZonelessChangeDetection } from '@angular/core';
import { provideHttpClient } from '@angular/common/http';
import { HttpTestingController, provideHttpClientTesting } from '@angular/common/http/testing';
import { provideRouter } from '@angular/router';

import { ProblemCheck, Problems as ProblemsData } from '../../models';
import { Problems } from './problems';

const CHECK: ProblemCheck = {
  source: 'mac-mini',
  collector: 'fleet-health',
  report_id: '01JR',
  section: 'isis',
  label: 'disk',
  subject: null,
  verdict: 'fail',
  observed: '96% used',
  expected: null,
  ref: null,
  collected_at: '2026-07-03T14:00:00Z',
};

function payload(overrides: Partial<ProblemsData> = {}): ProblemsData {
  return { checks: [CHECK], muted: [], stale: [], ...overrides };
}

/** Run scheduled effects so httpResource issues/settles its requests. NOT
 *  `whenStable()` — that awaits pending tasks, which include the very request
 *  the mock backend is holding open: a guaranteed deadlock. */
async function drain(): Promise<void> {
  await Promise.resolve();
  TestBed.tick();
  await Promise.resolve();
}

describe('Problems (mute flow)', () => {
  let fixture: ComponentFixture<Problems>;
  let cmp: Problems;
  let http: HttpTestingController;

  beforeEach(async () => {
    TestBed.configureTestingModule({
      providers: [
        provideZonelessChangeDetection(),
        provideRouter([]),
        provideHttpClient(),
        provideHttpClientTesting(),
      ],
    });
    http = TestBed.inject(HttpTestingController);
    fixture = TestBed.createComponent(Problems);
    cmp = fixture.componentInstance;
    await drain();
    http.expectOne('/api/problems').flush(payload());
    await drain();
  });

  afterEach(() => {
    fixture.destroy();
    http.verify();
    TestBed.resetTestingModule();
  });

  it('opens the mute form with fresh fields, and toggles it closed', () => {
    cmp.reason.set('leftover');
    cmp.toggleMute(CHECK);
    expect(cmp.openKey()).toBe('mac-mini fleet-health disk');
    expect(cmp.reason()).toBe(''); // a stale reason must not carry over
    cmp.toggleMute(CHECK);
    expect(cmp.openKey()).toBeNull();
  });

  it('submitting posts the mute and reloads the problems', async () => {
    cmp.toggleMute(CHECK);
    cmp.reason.set('drill scheduled for the weekend');
    cmp.submitMute(CHECK);

    const post = http.expectOne('/api/mutes');
    expect(post.request.method).toBe('POST');
    expect(post.request.body).toEqual({
      source: 'mac-mini',
      collector: 'fleet-health',
      label: 'disk',
      reason: 'drill scheduled for the weekend',
      ttl_hours: 24,
    });
    post.flush({});
    await drain();

    // The form closed and the list re-fetched — the muted check moves to the
    // "Muted" section server-side, so the client must re-ask, not guess.
    expect(cmp.openKey()).toBeNull();
    http.expectOne('/api/problems').flush(payload({ checks: [] }));
  });

  it('a blank reason never reaches the server', () => {
    cmp.toggleMute(CHECK);
    cmp.reason.set('   ');
    cmp.submitMute(CHECK);
    http.expectNone('/api/mutes');
  });

  it('a rejected mute keeps the form open for another try', async () => {
    cmp.toggleMute(CHECK);
    cmp.reason.set('why');
    cmp.submitMute(CHECK);
    http.expectOne('/api/mutes').flush('nope', { status: 422, statusText: 'Unprocessable' });
    await drain();
    expect(cmp.openKey()).toBe('mac-mini fleet-health disk');
    expect(cmp.saving()).toBe(false); // the button is usable again
  });

  it('unmuting deletes the mute and reloads', async () => {
    cmp.unmute('01JM');
    const del = http.expectOne('/api/mutes/01JM');
    expect(del.request.method).toBe('DELETE');
    del.flush(null);
    await drain();
    http.expectOne('/api/problems').flush(payload());
  });

  it('all clear fetches the overview to say what green covers', async () => {
    // Drain the seeded failure: reload to an empty problem set.
    cmp.data.reload();
    await drain();
    http.expectOne('/api/problems').flush(payload({ checks: [] }));
    await drain();

    expect(cmp.nothingWrong()).toBe(true);
    http.expectOne('/api/overview').flush([
      {
        source: 'mac-mini',
        collector: 'fleet-health',
        report_id: '01JR',
        collected_at: '2026-07-03T14:00:00Z',
        age_s: 120,
        interval_s: 3600,
        freshness: 'fresh',
        worst: 'pass',
        pass: 9,
        warn: 0,
        fail: 0,
        skip: 0,
        muted: 0,
        total: 9,
      },
    ]);
    await drain();
    expect(cmp.allClearDetail()).toBe('1 collector reporting · newest report 2m ago');
  });
});
