import { TestBed } from '@angular/core/testing';
import { APP_CONFIG, DEFAULT_APP_CONFIG } from './runtime-config';
import { ThemeService } from './theme.service';

describe('ThemeService', () => {
  beforeEach(() => {
    localStorage.clear();
    document.documentElement.removeAttribute('data-theme');
  });

  afterEach(() => {
    localStorage.clear();
    document.documentElement.removeAttribute('data-theme');
  });

  it('persists theme selection under the configured product key', () => {
    TestBed.configureTestingModule({
      providers: [
        {
          provide: APP_CONFIG,
          useValue: { ...DEFAULT_APP_CONFIG, themeStorageKey: 'stock-analysis-theme' },
        },
      ],
    });
    const service = TestBed.inject(ThemeService);

    service.setDark(true);

    expect(localStorage.getItem('stock-analysis-theme')).toBe('dark');
    expect(document.documentElement.getAttribute('data-theme')).toBe('dark');
  });

  it('migrates the legacy dark preference to the configured key', () => {
    localStorage.setItem('arc-theme', 'dark');
    TestBed.configureTestingModule({
      providers: [
        {
          provide: APP_CONFIG,
          useValue: { ...DEFAULT_APP_CONFIG, themeStorageKey: 'company-oa-theme' },
        },
      ],
    });

    const service = TestBed.inject(ThemeService);

    expect(service.isDark()).toBe(true);
    expect(localStorage.getItem('company-oa-theme')).toBe('dark');
  });
});
