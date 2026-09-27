import { Routes } from '@angular/router';

import { History } from './features/history/history';
import { Overview } from './features/overview/overview';
import { Problems } from './features/problems/problems';
import { Report } from './features/report/report';

// Eager components (the fleet forbids lazy loadComponent). The app is four small
// views, so there's nothing to code-split.
export const routes: Routes = [
  { path: '', component: Overview, title: 'Fleetwatch — overview' },
  // A peer of the overview, a tab of its own: no up.
  { path: 'problems', component: Problems, title: 'Fleetwatch — problems', data: { top: true } },
  // Opened from the overview and from problems, so up returns to whichever did.
  {
    path: 'reports/:id',
    component: Report,
    title: 'Fleetwatch — report',
    data: { up: { path: '/', opener: true } },
  },
  // Opened from a report and from problems.
  {
    path: 'history',
    component: History,
    title: 'Fleetwatch — history',
    data: { up: { path: '/problems', opener: true } },
  },
  { path: '**', redirectTo: '' },
];
