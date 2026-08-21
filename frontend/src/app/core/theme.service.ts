import { inject, Injectable, signal } from '@angular/core';
import { APP_CONFIG } from './runtime-config';

const LEGACY_STORAGE_KEY = 'arc-theme';

/** 主题服务:light / dark 手动切换,持久化到 localStorage */
@Injectable({ providedIn: 'root' })
export class ThemeService {
  readonly isDark = signal(false);
  private readonly storageKey = inject(APP_CONFIG).themeStorageKey;

  constructor() {
    const saved =
      typeof localStorage === 'undefined'
        ? null
        : (localStorage.getItem(this.storageKey) ?? localStorage.getItem(LEGACY_STORAGE_KEY));
    this.isDark.set(saved === 'dark');
    this.apply();
  }

  toggle(): void {
    this.isDark.update((v) => !v);
    this.apply();
  }

  setDark(dark: boolean): void {
    this.isDark.set(dark);
    this.apply();
  }

  private apply(): void {
    const root = document.documentElement;
    if (this.isDark()) {
      root.setAttribute('data-theme', 'dark');
      localStorage.setItem(this.storageKey, 'dark');
    } else {
      root.removeAttribute('data-theme');
      localStorage.setItem(this.storageKey, 'light');
    }
  }
}
