import { provideZonelessChangeDetection, signal } from '@angular/core';
import { ComponentFixture, TestBed } from '@angular/core/testing';
import { MatSnackBar } from '@angular/material/snack-bar';
import { ActivatedRoute, provideRouter } from '@angular/router';
import { vi } from 'vitest';
import { AuthService } from '../../core/auth.service';
import { DevRailApiService } from '../../features/devrail/data-access/devrail-api.service';
import { DEVRAIL_PERMISSIONS } from '../../features/devrail/devrail.permissions';
import type { DevRailRunResponse } from '../../generated/api/models';
import { DevRailRunPage } from './devrail-run';

const RUN: DevRailRunResponse = {
  id: 19,
  taskId: 7,
  taskRevision: 4,
  snapshotId: 3,
  idempotencyKey: 'scheduler:7:2',
  attempt: 2,
  actorType: 'system',
  cleanupStatus: 'completed',
  status: 'failed',
  cwd: '/tmp/devrail-test',
  policy: {},
  startupArgsSummary: [],
  recoveryAttempts: 1,
  retryReason: 'stall',
  recoverySuggestion: '系统将在退避结束后自动重试',
  workflowSource: 'repository',
  workflowVersion: '1',
  workflowDigest: 'a'.repeat(64),
  createdAt: '2026-08-26T00:00:00Z',
  updatedAt: '2026-08-26T00:01:00Z',
};

class EventSourceStub {
  onmessage: ((event: MessageEvent) => void) | null = null;
  onerror: (() => void) | null = null;
  readonly listeners = new Map<string, EventListenerOrEventListenerObject>();
  closed = false;

  addEventListener(type: string, listener: EventListenerOrEventListenerObject): void {
    this.listeners.set(type, listener);
  }

  close(): void {
    this.closed = true;
    this.listeners.clear();
  }
}

describe('DevRailRunPage', () => {
  let fixture: ComponentFixture<DevRailRunPage>;
  const permissionState = signal<ReadonlySet<string>>(new Set());
  const apiStub: Partial<DevRailApiService> = {
    getRun: vi.fn(async () => RUN),
    listRunEvents: vi.fn(async () => ({ items: [], nextCursor: null })),
    getRunChangeset: vi.fn(async () => ({ runId: 19, files: [] })),
    getRunQualityGates: vi.fn(async () => ({ runId: 19, items: [] })),
    listReviews: vi.fn(async () => ({ items: [], total: 0, page: 1, pageSize: 50 })),
  };

  beforeEach(async () => {
    permissionState.set(new Set());
    vi.stubGlobal('EventSource', EventSourceStub);
    await TestBed.configureTestingModule({
      imports: [DevRailRunPage],
      providers: [
        provideZonelessChangeDetection(),
        provideRouter([]),
        {
          provide: ActivatedRoute,
          useValue: { snapshot: { paramMap: { get: () => '19' } } },
        },
        {
          provide: AuthService,
          useValue: {
            hasPermission: (code: string) => permissionState().has(code),
          },
        },
        { provide: DevRailApiService, useValue: apiStub },
        { provide: MatSnackBar, useValue: { open: vi.fn() } },
      ],
    }).compileComponents();
    fixture = TestBed.createComponent(DevRailRunPage);
    await vi.waitFor(() => expect(fixture.componentInstance.loading()).toBe(false));
    await fixture.whenStable();
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('显示 attempt、系统 Actor、重试原因和清理结果', () => {
    const text = fixture.nativeElement.textContent as string;
    expect(text).toContain('执行尝试');
    expect(text).toContain('#2 · 系统调度器');
    expect(text).toContain('重试原因：stall');
    expect(text).toContain('清理状态');
    expect(text).toContain('任务修订号');
    expect(text).toContain('工作流来源');
    expect(text).toContain('repository');
    expect(text).toContain('工作流版本');
    expect(text).toContain('工作流摘要');
    expect(text).toContain('a'.repeat(64));
  });

  it('无运行权限时隐藏执行和重试操作', () => {
    const text = fixture.nativeElement.textContent as string;
    expect(text).not.toContain('执行质量门禁');
    expect(text).not.toContain('重试运行');
  });

  it('获得运行权限后显示执行和重试操作', async () => {
    permissionState.set(new Set([DEVRAIL_PERMISSIONS.runExecute, DEVRAIL_PERMISSIONS.runRetry]));
    await fixture.whenStable();
    const text = fixture.nativeElement.textContent as string;
    expect(text).toContain('执行质量门禁');
    expect(text).toContain('重试运行');
  });
});
