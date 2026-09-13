import {
  ApplicationConfig,
  LOCALE_ID,
  isDevMode,
  provideBrowserGlobalErrorListeners,
  provideZonelessChangeDetection,
} from '@angular/core';
import {
  provideHttpClient,
  withFetch,
  withInterceptors,
} from '@angular/common/http';
import { provideRouter, withComponentInputBinding } from '@angular/router';
import { provideServiceWorker } from '@angular/service-worker';

import { authRedirectInterceptor } from './auth-redirect.interceptor';
import { registerLocaleData } from '@angular/common';
import localeEnGb from '@angular/common/locales/en-GB';

import { routes } from './app.routes';

// Angular defaults LOCALE_ID to `en-US` whatever the browser is set to, so every
// `| date` rendered US dates to a UK reader. It is a different knob from
// `toLocaleString()`, which follows the browser and was already right on a phone
// — the two can disagree inside one render. Registering the locale data is
// required as well as naming the id: without it the pipe throws on any format
// needing month or day names.
registerLocaleData(localeEnGb);

export const appConfig: ApplicationConfig = {
  providers: [
    provideZonelessChangeDetection(),
    provideBrowserGlobalErrorListeners(),
    { provide: LOCALE_ID, useValue: 'en-GB' },
    // withComponentInputBinding: query/path params bind straight to component
    // inputs (the history view reads source/collector/section/label this way).
    provideRouter(routes, withComponentInputBinding()),
    provideHttpClient(withFetch(), withInterceptors([authRedirectInterceptor])),
    // Cache the app shell + last-seen status so the dashboard opens instantly
    // (and shows the last snapshot offline) — prod build only.
    provideServiceWorker('ngsw-worker.js', {
      enabled: !isDevMode(),
      registrationStrategy: 'registerWhenStable:30000',
    }),
  ],
};
