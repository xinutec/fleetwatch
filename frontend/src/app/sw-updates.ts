import { Injectable, inject } from '@angular/core';
import { SwUpdate, VersionReadyEvent } from '@angular/service-worker';
import {
  type PagePort,
  type ServiceWorkerPort,
  SwUpdates,
  type UpdateOutcome,
} from '@xinutec/ui-harness/sw-updates';
import { filter } from 'rxjs';

/** Session-scoped, so it survives the very reload it guards. */
const RECOVERY_KEY = 'fleetwatch.sw-recovery-attempted';

/**
 * Self-update, so a dashboard left open does not go on showing stale fleet status.
 *
 * ⚠ **ngsw alone caches an app that never learns a new build exists**, which is what
 * this repo had: the shell and the last snapshot are cached for offline use, and
 * nothing ever checked for a newer build. Stale monitoring is worse than none,
 * because it looks fine.
 *
 * The rules live in `@xinutec/ui-harness/sw-updates` — written and debugged in `life`,
 * the only app in the fleet that had them — and the one that matters most here is the
 * re-check on becoming visible: ngsw only re-checks at a navigation, which a resumed
 * long-lived tab never performs, and a dashboard left open for days IS that tab.
 *
 * This class is only the adapter. The policy is shared and unit-tested against a fake;
 * what is here is the Angular wiring, which is the part a test cannot reach.
 */
@Injectable({ providedIn: 'root' })
export class AppSwUpdates {
  private readonly sw = inject(SwUpdate);

  private readonly serviceWorker: ServiceWorkerPort = ((sw: SwUpdate) => ({
    // Bound to a local, not `this`: an object-literal getter does not capture the
    // enclosing `this` lexically, and a copied boolean would freeze `isEnabled` at
    // construction when start() must read the live value.
    get isEnabled(): boolean {
      return sw.isEnabled;
    },
    onVersionReady: (handler: () => void): void => {
      sw.versionUpdates
        .pipe(filter((event): event is VersionReadyEvent => event.type === 'VERSION_READY'))
        .subscribe(() => handler());
    },
    onUnrecoverable: (handler: () => void): void => {
      // The cached build is broken and the server no longer holds the files to
      // repair it — what a roll-forward deploy of :latest leaves a client whose
      // cache was evicted meanwhile. Only a fresh load escapes.
      sw.unrecoverable.subscribe(() => handler());
    },
    checkForUpdate: () => sw.checkForUpdate(),
    activateUpdate: () => sw.activateUpdate(),
  }))(this.sw);

  private readonly page: PagePort = {
    get hidden(): boolean {
      return document.visibilityState === 'hidden';
    },
    onVisibilityChange: (handler: () => void): void => {
      document.addEventListener('visibilitychange', handler);
    },
    recoveryAttempted: () => sessionStorage.getItem(RECOVERY_KEY) !== null,
    markRecoveryAttempted: () => sessionStorage.setItem(RECOVERY_KEY, '1'),
    reload: () => document.location.reload(),
    now: () => Date.now(),
  };

  private readonly policy = new SwUpdates(this.serviceWorker, this.page);

  start(): void {
    this.policy.start();
  }

  /** Manual "check for updates", for a settings screen to call. */
  checkNow(): Promise<UpdateOutcome> {
    return this.policy.checkNow();
  }
}
