import { ChangeDetectionStrategy, Component, inject, signal } from '@angular/core';
import { RouterLink, RouterLinkActive, RouterOutlet } from '@angular/router';
import { MatBadgeModule } from '@angular/material/badge';
import { MatIconModule } from '@angular/material/icon';

import { BUILD_INFO } from './build-info';
import { ProblemsStore } from './problems-store';
import { Telemetry } from './telemetry';

@Component({
  selector: 'app-root',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [RouterOutlet, RouterLink, RouterLinkActive, MatBadgeModule, MatIconModule],
  templateUrl: './app.html',
  styleUrl: './app.scss',
})
export class App {
  readonly build = BUILD_INFO;

  /**
   * The client activity trace. Started here and referred to nowhere else: it
   * subscribes to the router and taps clicks globally, so no screen has to
   * remember to join — and a trace each screen must opt into is one with holes
   * in exactly the screens nobody thought about.
   */
  private readonly telemetry = inject(Telemetry);

  // Standing badge on the Problems tab, fed by the shared store — the same
  // resource the problems page shows and refreshes, so the badge stays live.
  readonly problemCount = inject(ProblemsStore).count;

  // The nav is a bottom tab bar on phones, a left rail from tablet up.
  readonly tabs = signal([
    { path: '/', exact: true, icon: 'dashboard', label: 'Overview' },
    { path: '/problems', exact: false, icon: 'warning', label: 'Problems' },
  ]);

  constructor() {
    // After the field initialisers, so the router this subscribes to exists.
    // Idempotent, so a shell recreated in a test does not stack listeners.
    this.telemetry.init();
  }
}
