import { DEFAULT_APP_CONFIG, resolveRuntimeConfig } from './runtime-config';

describe('runtime product config', () => {
  it('uses stable defaults when no deployment config is provided', () => {
    const config = resolveRuntimeConfig(undefined);

    expect(config).toEqual(DEFAULT_APP_CONFIG);
    expect(Object.isFrozen(config)).toBe(true);
  });

  it('normalizes deployment values and removes a trailing API slash', () => {
    const config = resolveRuntimeConfig({
      appName: ' 股票分析系统 ',
      appShortName: ' 投研平台 ',
      appSlug: ' stock-analysis ',
      apiBaseUrl: ' https://api.example.test/v1/ ',
      themeStorageKey: ' stock-theme ',
    });

    expect(config).toEqual({
      appName: '股票分析系统',
      appShortName: '投研平台',
      appSlug: 'stock-analysis',
      apiBaseUrl: 'https://api.example.test/v1',
      themeStorageKey: 'stock-theme',
    });
  });

  it('derives a theme key from the product slug', () => {
    const config = resolveRuntimeConfig({ appSlug: 'company-oa', themeStorageKey: ' ' });

    expect(config.themeStorageKey).toBe('company-oa-theme');
  });

  it('uses a custom product name as the short name when none is provided', () => {
    const config = resolveRuntimeConfig({ appName: '公司办公系统' });

    expect(config.appShortName).toBe('公司办公系统');
  });
});
