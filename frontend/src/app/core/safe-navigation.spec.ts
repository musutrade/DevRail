import { safeDevRailRoute, safeDownloadFileName, safeDownloadUrl } from './safe-navigation';

describe('safe navigation values', () => {
  it('accepts same-origin artifact URLs and rejects active or external protocols', () => {
    expect(safeDownloadUrl('/api/v1/artifacts/7/download')).toContain(
      '/api/v1/artifacts/7/download',
    );
    expect(safeDownloadUrl('http://localhost/api/v1/artifacts/7/download')).toBeNull();
    expect(safeDownloadUrl('/\\evil.example/artifact')).toBeNull();
    expect(safeDownloadUrl('javascript:alert(1)')).toBeNull();
    expect(safeDownloadUrl('https://evil.example/artifact')).toBeNull();
  });

  it('accepts normalized file names without paths', () => {
    expect(safeDownloadFileName('devrail-run-7.patch')).toBe('devrail-run-7.patch');
    expect(safeDownloadFileName('../secret.txt')).toBeNull();
    expect(safeDownloadFileName('报告.txt')).toBeNull();
  });

  it('accepts only known DevRail resource routes', () => {
    expect(safeDevRailRoute('/devrail/runs/7')).toBe('/devrail/runs/7');
    expect(safeDevRailRoute('/devrail/projects/3/tasks/9')).toBe('/devrail/projects/3/tasks/9');
    expect(safeDevRailRoute('https://evil.example')).toBeNull();
    expect(safeDevRailRoute('/admin/users')).toBeNull();
  });
});
