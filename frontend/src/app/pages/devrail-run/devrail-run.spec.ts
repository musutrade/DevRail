import { provideZonelessChangeDetection, signal } from '@angular/core';
import { ComponentFixture, TestBed } from '@angular/core/testing';
import { MatSnackBar } from '@angular/material/snack-bar';
import { ActivatedRoute, provideRouter } from '@angular/router';
import { vi } from 'vitest';
import { AuthService } from '../../core/auth.service';
import { DevRailApiService } from '../../features/devrail/data-access/devrail-api.service';
import { DEVRAIL_PERMISSIONS } from '../../features/devrail/devrail.permissions';
import type {
  DevRailContinuationResponse,
  DevRailRepairResponse,
  DevRailRunResponse,
} from '../../generated/api/models';
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
  runKind: 'retry',
  handoffEvidenceStatus: 'available',
  status: 'failed',
  cwd: '/tmp/devrail-test',
  policy: {},
  startupArgsSummary: [],
  recoveryAttempts: 1,
  retryReason: 'stall',
  recoverySuggestion: '系统将在退避结束后自动重试',
  parentRunId: 11,
  parentTurnId: 'turn-parent',
  workflowSource: 'repository',
  workflowVersion: '1',
  workflowDigest: 'a'.repeat(64),
  createdAt: '2026-08-26T00:00:00Z',
  updatedAt: '2026-08-26T00:01:00Z',
};

const CONTINUATION: DevRailContinuationResponse = {
  id: 31,
  taskId: 7,
  sourceRunId: 19,
  rootRunId: 19,
  sourceTurnId: 'turn-19',
  triggerType: 'review_changes',
  contextSummary: '根据审查意见继续修改',
  continuationSequence: 1,
  chainDepth: 1,
  status: 'claimed',
  createdAt: '2026-08-26T00:02:00Z',
  updatedAt: '2026-08-26T00:02:00Z',
};

const REPAIR: DevRailRepairResponse = {
  id: 41,
  taskId: 7,
  sourceRunId: 19,
  rootRunId: 11,
  diagnosisId: 51,
  repairSequence: 1,
  riskCategory: 'low_risk',
  strategyVersion: 'repair-policy-v1',
  status: 'running',
  costUnits: 1,
  diagnosis: {
    id: 51,
    sourceRunId: 19,
    evidenceRef: 'quality-gate:19:1',
    evidenceObservedAt: '2026-08-26T00:03:00Z',
    affectedGates: ['backend_tests'],
    errorSummary: '后端质量门禁未通过',
    structuredError: { code: 'quality_gate_failed' },
    environmentSummary: { source: 'quality_gate' },
    createdAt: '2026-08-26T00:03:00Z',
  },
  gateReruns: [],
  handoffs: [],
  createdAt: '2026-08-26T00:03:00Z',
  updatedAt: '2026-08-26T00:03:00Z',
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
    listContinuations: vi.fn(async () => ({
      items: [CONTINUATION],
      total: 1,
      page: 1,
      pageSize: 50,
    })),
    listRepairs: vi.fn(async () => ({ items: [REPAIR], total: 1, page: 1, pageSize: 50 })),
    getRepair: vi.fn(async () => REPAIR),
    cancelContinuation: vi.fn(async () => ({ ...CONTINUATION, status: 'cancelled' as const })),
  };

  beforeEach(async () => {
    vi.clearAllMocks();
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
    expect(text).toContain('可用于追加执行');
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

  it('区分运行类型并仅按 continuation 权限取消未启动请求', async () => {
    permissionState.set(new Set([DEVRAIL_PERMISSIONS.continuationCancel]));
    await fixture.whenStable();
    const page = fixture.componentInstance;
    expect(fixture.nativeElement.textContent).toContain('失败重试');
    expect(fixture.nativeElement.textContent).toContain('根据审查意见继续修改');
    await page.cancelPendingContinuation(CONTINUATION);
    expect(page.continuations()[0].status).toBe('cancelled');
  });

  it('展示受控修复序号、诊断和门禁范围', () => {
    const text = fixture.nativeElement.textContent as string;
    expect(text).toContain('受控修复谱系');
    expect(text).toContain('第 1 次');
    expect(text).toContain('后端质量门禁未通过');
    expect(text).toContain('backend_tests');
  });

  it('展示门禁重跑结果、来源/子运行链接和人工交接原因', async () => {
    const page = fixture.componentInstance;
    page.repair.set({
      ...REPAIR,
      status: 'handed_off',
      childRunId: 27,
      handoffReason: 'hook_failure_circuit_open',
      gateReruns: [
        {
          id: 81,
          gateId: 'backend_tests',
          changesetDigest: 'b'.repeat(64),
          status: 'failed',
          resultCode: 'command_failed',
          summary: '后端测试仍未通过',
          logRef: 'quality-gates/backend-tests',
          durationMs: 120,
          childRunId: 27,
          createdAt: '2026-08-26T00:04:00Z',
          completedAt: '2026-08-26T00:05:00Z',
        },
      ],
    });
    await fixture.whenStable();
    const text = fixture.nativeElement.textContent as string;
    expect(text).toContain('后端测试仍未通过');
    expect(text).toContain('交接原因：Hook 失败熔断已打开');
    expect(text).toContain('查看来源运行 #19');
    expect(text).toContain('查看修复子运行 #27');
  });

  it('展示父运行和父回合，并拒绝取消已派发请求', async () => {
    permissionState.set(new Set([DEVRAIL_PERMISSIONS.continuationCancel]));
    const page = fixture.componentInstance;
    page.continuations.set([{ ...CONTINUATION, status: 'dispatched' }]);
    await fixture.whenStable();
    const text = fixture.nativeElement.textContent as string;
    expect(text).toContain('父运行');
    expect(text).toContain('父回合');
    expect(text).toContain('turn-parent');
    await page.cancelPendingContinuation({ ...CONTINUATION, status: 'dispatched' });
    expect(apiStub.cancelContinuation).not.toHaveBeenCalled();
  });
});
