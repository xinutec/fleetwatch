import { ChangeDetectionStrategy, Component, inject, signal } from '@angular/core';
import { RouterLink, RouterLinkActive, RouterOutlet } from '@angular/router';
import { MatBadgeModule } from '@angular/material/badge';
import { MatIconModule } from '@angular/material/icon';

import { BUILD_INFO } from './build-info';
import { ProblemsStore } from './problems-store';

@Component({
  selector: 'app-root',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [RouterOutlet, RouterLink, RouterLinkActive, MatBadgeModule, MatIconModule],
  templateUrl: './app.html',
  styleUrl: './app.scss',
})
export class App {
  readonly build = BUILD_INFO;

  // Standing badge on the Problems tab, fed by the shared store — the same
  // resource the problems page shows and refreshes, so the badge stays live.
  readonly problemCount = inject(ProblemsStore).count;

  // The nav is a bottom tab bar on phones, a left rail from tablet up.
  readonly tabs = signal([
    { path: '/', exact: true, icon: 'dashboard', label: 'Overview' },
    { path: '/problems', exact: false, icon: 'warning', label: 'Problems' },
  ]);
}
