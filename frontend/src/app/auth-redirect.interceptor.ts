import { HttpErrorResponse, HttpInterceptorFn } from '@angular/common/http';
import { catchError, throwError } from 'rxjs';

// The read API is gated by a Nextcloud-login session (see the backend's
// AuthUser extractor). When a call comes back 401 the browser has no valid
// session, so bounce to /login — a top-level backend route that kicks off the
// NC OAuth flow and returns here. /login is not under /api, so this interceptor
// (which only wraps HttpClient calls) never loops on the redirect itself.
//
// dev-lint: allow-http-status — this interceptor IS fleetwatch's HTTP error
// boundary. It acts only on an exact 401 (an offline status 0 falls through and
// rethrows, so there's no offline-vs-auth confusion to classify); a shared
// discriminated-union classifier would be over-engineering for a read-only app.
export const authRedirectInterceptor: HttpInterceptorFn = (req, next) =>
  next(req).pipe(
    catchError((err: HttpErrorResponse) => {
      if (err.status === 401) {
        const returnTo = location.pathname + location.search;
        location.href = `/login?return_to=${encodeURIComponent(returnTo)}`;
      }
      return throwError(() => err);
    }),
  );
