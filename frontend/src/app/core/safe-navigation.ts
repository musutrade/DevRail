const SAFE_FILE_NAME = /^[A-Za-z0-9._-]{1,128}$/;
const SAFE_DEVRAIL_ROUTE =
  /^\/devrail\/(?:runs|approvals)\/[1-9]\d*$|^\/devrail\/projects\/[1-9]\d*\/(?:tasks|repositories)\/[1-9]\d*$/;

export function safeDownloadUrl(value: string): string | null {
  const trimmed = value.trim();
  if (!trimmed || hasUnsafeUrlCharacters(trimmed) || trimmed.includes('\\')) return null;
  const relativePath = trimmed.startsWith('/') && !trimmed.startsWith('//');
  try {
    const url = new URL(trimmed, window.location.origin);
    if (relativePath && !url.pathname.startsWith('/')) return null;
    if (url.origin !== window.location.origin) {
      return null;
    }
    if (relativePath) return trimmed;
    return url.protocol === 'https:' ? url.href : null;
  } catch {
    return null;
  }
}

function hasUnsafeUrlCharacters(value: string): boolean {
  return [...value].some((character) => {
    const code = character.charCodeAt(0);
    return code <= 0x1f || code === 0x7f;
  });
}

export function safeDownloadFileName(value: string): string | null {
  const trimmed = value.trim();
  return SAFE_FILE_NAME.test(trimmed) && !['.', '..'].includes(trimmed) ? trimmed : null;
}

export function safeDevRailRoute(value: string | null | undefined): string | null {
  const trimmed = value?.trim();
  return trimmed && SAFE_DEVRAIL_ROUTE.test(trimmed) ? trimmed : null;
}
