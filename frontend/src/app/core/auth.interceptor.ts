import { DOCUMENT } from '@angular/common';
import { HttpErrorResponse, HttpInterceptorFn } from '@angular/common/http';
import { inject } from '@angular/core';
import { Router } from '@angular/router';
import { catchError, throwError } from 'rxjs';
import { AuthService } from './auth.service';
import { API_BASE_URL } from './runtime-config';

const CSRF_COOKIE_NAMES = ['__Host-arc_csrf', 'arc_csrf'];
const SAFE_METHODS = new Set(['GET', 'HEAD', 'OPTIONS']);

export function readCsrfToken(cookieHeader: string): string | null {
  const cookies = new Map<string, string>();
  for (const part of cookieHeader.split(';')) {
    const separator = part.indexOf('=');
    if (separator < 0) {
      continue;
    }
    const name = part.slice(0, separator).trim();
    if (CSRF_COOKIE_NAMES.includes(name)) {
      try {
        cookies.set(name, decodeURIComponent(part.slice(separator + 1)));
      } catch {
        continue;
      }
    }
  }
  return CSRF_COOKIE_NAMES.map((name) => cookies.get(name)).find(Boolean) ?? null;
}

export const authInterceptor: HttpInterceptorFn = (request, next) => {
  const auth = inject(AuthService);
  const router = inject(Router);
  const apiBaseUrl = inject(API_BASE_URL);
  const document = inject(DOCUMENT);
  const targetsApi = request.url === apiBaseUrl || request.url.startsWith(`${apiBaseUrl}/`);
  let authenticatedRequest = request;

  if (targetsApi) {
    const csrfToken = readCsrfToken(document.cookie);
    authenticatedRequest = request.clone({
      withCredentials: true,
      setHeaders:
        !SAFE_METHODS.has(request.method) && csrfToken ? { 'X-CSRF-Token': csrfToken } : {},
    });
  }

  return next(authenticatedRequest).pipe(
    catchError((error: unknown) => {
      if (error instanceof HttpErrorResponse && error.status === 401) {
        auth.handleUnauthorized();
        if (!request.url.endsWith('/auth/login')) {
          void router.navigate(['/login']);
        }
      }
      return throwError(() => error);
    }),
  );
};
