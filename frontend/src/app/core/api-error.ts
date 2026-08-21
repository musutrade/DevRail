import { HttpErrorResponse } from '@angular/common/http';

export function apiErrorMessage(error: unknown, fallback: string): string {
  if (!(error instanceof HttpErrorResponse)) {
    return fallback;
  }
  const body = error.error as
    { error?: { message?: unknown; traceId?: unknown } } | null | undefined;
  const candidate = body?.error?.message;
  const message = typeof candidate === 'string' && candidate.length > 0 ? candidate : fallback;
  if (error.status < 500) {
    return message;
  }
  const bodyTraceId = body?.error?.traceId;
  const headerTraceId = error.headers.get('x-request-id');
  const traceId = validTraceId(bodyTraceId)
    ? bodyTraceId
    : validTraceId(headerTraceId)
      ? headerTraceId
      : null;
  return traceId ? `${message}（问题编号：${traceId}）` : message;
}

function validTraceId(value: unknown): value is string {
  return (
    typeof value === 'string' &&
    value.length > 0 &&
    value.length <= 64 &&
    /^[A-Za-z0-9._-]+$/.test(value)
  );
}
