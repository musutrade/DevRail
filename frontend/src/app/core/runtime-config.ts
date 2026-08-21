import { inject, InjectionToken } from '@angular/core';

export interface AppRuntimeConfig {
  readonly appName: string;
  readonly appShortName: string;
  readonly appSlug: string;
  readonly apiBaseUrl: string;
  readonly themeStorageKey: string;
}

declare global {
  interface Window {
    __ARC_ADMIN_CONFIG__?: Partial<AppRuntimeConfig>;
  }
}

export const DEFAULT_APP_CONFIG: AppRuntimeConfig = Object.freeze({
  appName: 'RBAC 管理中心',
  appShortName: 'RBAC',
  appSlug: 'arc-admin',
  apiBaseUrl: '/api/v1',
  themeStorageKey: 'arc-admin-theme',
});

function valueOrDefault(value: string | undefined, fallback: string): string {
  return value?.trim() || fallback;
}

export function resolveRuntimeConfig(
  source: Partial<AppRuntimeConfig> | undefined,
): AppRuntimeConfig {
  const appSlug = valueOrDefault(source?.appSlug, DEFAULT_APP_CONFIG.appSlug);
  const appName = valueOrDefault(source?.appName, DEFAULT_APP_CONFIG.appName);
  const apiBaseUrl = valueOrDefault(source?.apiBaseUrl, DEFAULT_APP_CONFIG.apiBaseUrl).replace(
    /\/+$/,
    '',
  );

  return Object.freeze({
    appName,
    appShortName: valueOrDefault(
      source?.appShortName,
      source?.appName?.trim() ? appName : DEFAULT_APP_CONFIG.appShortName,
    ),
    appSlug,
    apiBaseUrl,
    themeStorageKey: valueOrDefault(source?.themeStorageKey, `${appSlug}-theme`),
  });
}

export const APP_CONFIG = new InjectionToken<AppRuntimeConfig>('APP_CONFIG', {
  providedIn: 'root',
  factory: () =>
    resolveRuntimeConfig(typeof window === 'undefined' ? undefined : window.__ARC_ADMIN_CONFIG__),
});

export const API_BASE_URL = new InjectionToken<string>('API_BASE_URL', {
  providedIn: 'root',
  factory: () => inject(APP_CONFIG).apiBaseUrl,
});
