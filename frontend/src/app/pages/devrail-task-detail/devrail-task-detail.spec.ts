import { provideZonelessChangeDetection, signal } from '@angular/core';
import { ComponentFixture, TestBed } from '@angular/core/testing';
import { MatSnackBar } from '@angular/material/snack-bar';
import { ActivatedRoute, provideRouter } from '@angular/router';
import { vi } from 'vitest';
import { AuthService } from '../../core/auth.service';
import { DevRailApiService } from '../../features/devrail/data-access/devrail-api.service';
import type { DevRailTaskResponse } from '../../generated/api/models';
import { DevRailTaskDetailPage } from './devrail-task-detail';
import type {
  DevRailContinuationResponse,
  DevRailRepairResponse,
  DevRailRunResponse,
} from '../../generated/api/models';

const TASK: DevRailTaskResponse = {
  id: 7,
  projectId: 3,
  organizationId: 2,
  ownerUserId: 5,
  title: '验证工作流快照',
  goal: '确认排队任务使用不可变工作流',
  labels: [],
  priority: 'high',
  status: 'queued',
  revision: 4,
  schedulerAttempt: 1,
  schedulerMaxAttempts: 3,
  schedulerRetryCount: 0,
  workflowSource: 'repository',
  workflowVersion: '1',
  workflowDigest: 'b'.repeat(64),
  continuationPolicy: {
    enabled: true,
    max_context_bytes: 16_384,
  },
  repairPolicy: {
    enabled: true,
    max_repairs: 3,
    max_cost_units: 10,
    max_diagnostic_bytes: 4096,
    evidence_max_age_seconds: 3600,
    claim_lease_seconds: 60,
    max_dispatch_attempts: 3,
    retry_base_delay_seconds: 10,
    retry_max_delay_seconds: 300,
    auto_categories: ['low_risk'],
    approval_categories: ['logical_change'],
  },
  continuationCapabilities: {
    canRead: true,
    canCreate: true,
    canCancel: true,
  },
  creationSource: 'manual',
  followupDepth: 0,
  prerequisites: [],
  dependents: [],
  createdAt: '2026-08-26T00:00:00Z',
  updatedAt: '2026-08-26T00:01:00Z',
};

const SOURCE_RUN: DevRailRunResponse = {
  id: 19,
  taskId: 7,
  taskRevision: 4,
  snapshotId: 3,
  idempotencyKey: 'source-19',
  attempt: 1,
  actorType: 'user',
  cleanupStatus: 'completed',
  status: 'completed',
  runKind: 'primary',
  cwd: '/controlled/workspace',
  policy: {},
  startupArgsSummary: [],
  recoveryAttempts: 0,
  threadId: 'thread-19',
  turnId: 'turn-19',
  workflowSource: 'repository',
  workflowVersion: '1',
  workflowDigest: 'b'.repeat(64),
  createdAt: '2026-08-26T00:00:00Z',
  updatedAt: '2026-08-26T00:01:00Z',
};

const CONTINUATION: DevRailContinuationResponse = {
  id: 31,
  taskId: 7,
  sourceRunId: 19,
  rootRunId: 19,
  sourceTurnId: 'turn-19',
  triggerType: 'user_context',
  contextSummary: '补充验证失败场景',
  continuationSequence: 1,
  chainDepth: 1,
  status: 'pending',
  childRunId: null,
  resultCode: null,
  createdAt: '2026-08-26T00:02:00Z',
  updatedAt: '2026-08-26T00:02:00Z',
};

const REPAIR: DevRailRepairResponse = {
  id: 41,
  taskId: 7,
  sourceRunId: 19,
  rootRunId: 19,
  diagnosisId: 12,
  repairSequence: 1,
  riskCategory: 'low_risk',
  strategyVersion: 'repair-policy-v1',
  status: 'pending',
  costUnits: 1,
  diagnosis: {
    id: 12,
    sourceRunId: 19,
    evidenceRef: 'quality-gate:19:1',
    evidenceObservedAt: '2026-08-26T00:02:00Z',
    affectedGates: ['backend_tests'],
    errorSummary: '质量门禁未通过：backend_tests',
    structuredError: { code: 'quality_gate_failed' },
    environmentSummary: { source: 'quality_gate' },
    createdAt: '2026-08-26T00:02:00Z',
  },
  gateReruns: [
    {
      id: 51,
      gateId: 'backend_tests',
      changesetDigest: 'a'.repeat(64),
      status: 'passed',
      createdAt: '2026-08-26T00:03:00Z',
    },
  ],
  handoffs: [],
  createdAt: '2026-08-26T00:02:00Z',
  updatedAt: '2026-08-26T00:02:00Z',
};

class EventSourceStub {
  static instances: EventSourceStub[] = [];
  onmessage: ((event: MessageEvent) => void) | null = null;
  onerror: (() => void) | null = null;
  closed = false;
  readonly url: string;

  constructor(url: string | URL) {
    this.url = String(url);
    EventSourceStub.instances.push(this);
  }

  addEventListener(_type: string, _listener: EventListenerOrEventListenerObject): void {
    void _type;
    void _listener;
  }

  close(): void {
    this.closed = true;
  }
}

describe('DevRailTaskDetailPage', () => {
  let fixture: ComponentFixture<DevRailTaskDetailPage>;
  let apiStub: Partial<DevRailApiService>;
  const permissionState = signal<ReadonlySet<string>>(new Set());

  beforeEach(async () => {
    EventSourceStub.instances = [];
    vi.stubGlobal('EventSource', EventSourceStub);
    apiStub = {
      getTask: vi.fn(async () => TASK),
      listRepositories: vi.fn(async () => ({ items: [], total: 0, page: 1, pageSize: 20 })),
      listEnvironments: vi.fn(async () => ({ items: [], total: 0, page: 1, pageSize: 20 })),
      listTaskComments: vi.fn(async () => ({ items: [], total: 0, page: 1, pageSize: 50 })),
      listTaskEvents: vi.fn(async () => ({ items: [], nextCursor: null })),
      listTasks: vi.fn(async () => ({ items: [], total: 0, page: 1, pageSize: 100 })),
      listRuns: vi.fn(async () => ({ items: [SOURCE_RUN], total: 1, page: 1, pageSize: 100 })),
      listContinuations: vi.fn(async () => ({ items: [], total: 0, page: 1, pageSize: 100 })),
      createContinuation: vi.fn(async () => CONTINUATION),
      cancelContinuation: vi.fn(async () => ({ ...CONTINUATION, status: 'cancelled' as const })),
      listRepairs: vi.fn(async () => ({ items: [REPAIR], total: 1, page: 1, pageSize: 100 })),
      createRepair: vi.fn(async () => REPAIR),
      cancelRepair: vi.fn(async () => ({ ...REPAIR, status: 'cancelled' as const })),
      handoffRepair: vi.fn(async () => ({ ...REPAIR, status: 'handed_off' as const })),
      retryRepair: vi.fn(async () => ({
        ...REPAIR,
        id: 42,
        repairSequence: 2,
        status: 'pending' as const,
      })),
      approveRepair: vi.fn(async () => ({
        id: 91,
        repairRequestId: REPAIR.id,
        riskCategory: 'logical_change' as const,
        policyVersion: 'repair-policy-v1',
        status: 'approved' as const,
        requestedBy: 5,
        decidedBy: 5,
        decisionReason: null,
        expiresAt: '2026-08-26T01:00:00Z',
        createdAt: '2026-08-26T00:02:00Z',
        updatedAt: '2026-08-26T00:03:00Z',
      })),
      rejectRepair: vi.fn(async () => ({
        id: 91,
        repairRequestId: REPAIR.id,
        riskCategory: 'logical_change' as const,
        policyVersion: 'repair-policy-v1',
        status: 'rejected' as const,
        requestedBy: 5,
        decidedBy: 5,
        decisionReason: '人工审核未通过',
        expiresAt: '2026-08-26T01:00:00Z',
        createdAt: '2026-08-26T00:02:00Z',
        updatedAt: '2026-08-26T00:03:00Z',
      })),
      withdrawRepairApproval: vi.fn(async () => ({
        id: 91,
        repairRequestId: REPAIR.id,
        riskCategory: 'logical_change' as const,
        policyVersion: 'repair-policy-v1',
        status: 'withdrawn' as const,
        requestedBy: 5,
        decidedBy: 5,
        decisionReason: '申请人主动撤回审批',
        expiresAt: '2030-01-01T01:00:00Z',
        createdAt: '2026-08-26T00:02:00Z',
        updatedAt: '2026-08-26T00:03:00Z',
      })),
    };

    await TestBed.configureTestingModule({
      imports: [DevRailTaskDetailPage],
      providers: [
        provideZonelessChangeDetection(),
        provideRouter([]),
        {
          provide: ActivatedRoute,
          useValue: {
            snapshot: {
              paramMap: { get: (name: string) => (name === 'projectId' ? '3' : '7') },
            },
          },
        },
        { provide: DevRailApiService, useValue: apiStub },
        {
          provide: AuthService,
          useValue: {
            hasPermission: (code: string) => permissionState().has(code),
            currentUser: () => ({ id: 5 }),
          },
        },
        { provide: MatSnackBar, useValue: { open: vi.fn() } },
      ],
    }).compileComponents();

    fixture = TestBed.createComponent(DevRailTaskDetailPage);
    await vi.waitFor(() => expect(fixture.componentInstance.loading()).toBe(false));
    await fixture.whenStable();
  });

  afterEach(() => {
    permissionState.set(new Set());
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  it('显示任务修订号和工作流身份', () => {
    const text = fixture.nativeElement.textContent as string;
    expect(text).toContain('派发契约');
    expect(text).toContain('任务修订号：4');
    expect(text).toContain('工作流来源：repository');
    expect(text).toContain('工作流版本：1');
    expect(text).toContain(`工作流摘要：${'b'.repeat(64)}`);
  });

  it('无依赖写权限时只显示关系且不显示编辑器', () => {
    expect(fixture.nativeElement.textContent).toContain('暂无前置任务');
    expect(fixture.nativeElement.textContent).not.toContain('编辑前置任务');
  });

  it('依赖编辑器拒绝自依赖和重复依赖', () => {
    const page = fixture.componentInstance;
    page.dependencyCandidateId.set(7);
    page.addDependency();
    expect(page.dependencyDraft()).toEqual([]);
    page.dependencyCandidateId.set(8);
    page.addDependency();
    page.dependencyCandidateId.set(8);
    page.addDependency();
    expect(page.dependencyDraft().map((item) => item.prerequisiteTaskId)).toEqual([8]);
  });

  it('展示追加执行时间线并按权限提交和取消请求', async () => {
    const page = fixture.componentInstance;
    permissionState.set(new Set(['devrail:continuation:create', 'devrail:continuation:cancel']));
    page.task.set({ ...TASK, status: 'succeeded' });
    page.runs.set([SOURCE_RUN]);
    page.continuations.set([CONTINUATION]);
    await fixture.whenStable();
    expect(fixture.nativeElement.textContent).toContain('追加执行');
    expect(fixture.nativeElement.textContent).toContain('补充验证失败场景');
    const form = fixture.nativeElement.querySelector('.continuation-form') as HTMLFormElement;
    const textarea = form.querySelector('textarea') as HTMLTextAreaElement;
    textarea.value = '请补充边界测试';
    textarea.dispatchEvent(new Event('input'));
    await fixture.whenStable();
    await page.createContinuation(form);
    expect(page.continuations()[0].status).toBe('pending');
    await page.cancelContinuation(CONTINUATION);
    expect(page.continuations()[0].status).toBe('cancelled');
  });

  it('策略关闭时隐藏表单并显示中文原因', async () => {
    permissionState.set(new Set(['devrail:continuation:create']));
    fixture.componentInstance.task.set({
      ...TASK,
      status: 'succeeded',
      continuationPolicy: { ...TASK.continuationPolicy, enabled: false },
    });
    fixture.componentInstance.runs.set([SOURCE_RUN]);
    await fixture.whenStable();
    expect(fixture.nativeElement.querySelector('.continuation-form')).toBeNull();
    expect(fixture.nativeElement.textContent).toContain('当前任务策略未启用追加执行');
  });

  it('追加执行空态使用中文，并阻止重复提交', async () => {
    permissionState.set(new Set(['devrail:continuation:create']));
    const page = fixture.componentInstance;
    page.task.set({ ...TASK, status: 'succeeded' });
    page.runs.set([SOURCE_RUN]);
    page.continuations.set([]);
    await fixture.whenStable();
    expect(fixture.nativeElement.textContent).toContain('暂无追加执行记录');
    const form = fixture.nativeElement.querySelector('.continuation-form') as HTMLFormElement;
    page.continuationInput.set('请复核边界条件');
    const first = page.createContinuation(form);
    const second = page.createContinuation(form);
    await Promise.all([first, second]);
    expect(apiStub.createContinuation).toHaveBeenCalledTimes(1);
  });

  it('展示脱敏修复诊断、门禁结果和低风险修复入口', async () => {
    permissionState.set(new Set(['devrail:repair:create']));
    const page = fixture.componentInstance;
    page.task.set({ ...TASK, status: 'failed' });
    page.runs.set([{ ...SOURCE_RUN, status: 'failed' }]);
    page.repairs.set([REPAIR]);
    await fixture.whenStable();
    const text = fixture.nativeElement.textContent as string;
    expect(text).toContain('受控修复');
    expect(text).toContain('质量门禁未通过：backend_tests');
    expect(text).toContain('受影响门禁：backend_tests');
    expect(text).toContain('请求低风险修复');
    await page.createRepair();
    expect(apiStub.createRepair).toHaveBeenCalledWith(
      19,
      expect.objectContaining({ riskCategory: 'low_risk' }),
    );
  });

  it('修复区域提供加载语义、动态更新和操作后焦点恢复', async () => {
    permissionState.set(new Set(['devrail:repair:cancel']));
    const page = fixture.componentInstance;
    page.task.set({ ...TASK, status: 'failed' });
    page.runs.set([{ ...SOURCE_RUN, status: 'failed' }]);
    page.repairs.set([REPAIR]);
    await fixture.whenStable();

    const section = fixture.nativeElement.querySelector('.repair-section') as HTMLElement;
    const heading = fixture.nativeElement.querySelector('#repair-title') as HTMLElement;
    const list = fixture.nativeElement.querySelector('.repair-list') as HTMLElement;
    expect(section.getAttribute('aria-busy')).toBe('false');
    expect(heading.getAttribute('tabindex')).toBe('-1');
    expect(list.getAttribute('aria-live')).toBe('polite');

    await page.cancelRepair(REPAIR);
    await fixture.whenStable();
    await Promise.resolve();
    expect(document.activeElement).toBe(heading);
  });

  it('事件流断开后重连，并在组件销毁后清除待执行重连', () => {
    const source = EventSourceStub.instances.at(-1);
    expect(source?.url).toBe('/api/v1/tasks/7/events/stream');

    vi.useFakeTimers();
    source?.onerror?.();
    vi.advanceTimersByTime(1_500);
    expect(EventSourceStub.instances).toHaveLength(2);

    const reconnected = EventSourceStub.instances.at(-1);
    reconnected?.onerror?.();
    fixture.destroy();
    vi.advanceTimersByTime(1_500);
    expect(EventSourceStub.instances).toHaveLength(2);
  });

  it('策略关闭时隐藏修复入口且不调用创建接口', async () => {
    permissionState.set(new Set(['devrail:repair:create']));
    const page = fixture.componentInstance;
    page.task.set({
      ...TASK,
      status: 'failed',
      repairPolicy: { ...TASK.repairPolicy, enabled: false },
    });
    page.runs.set([{ ...SOURCE_RUN, status: 'failed' }]);
    page.repairs.set([]);
    await fixture.whenStable();
    expect(fixture.nativeElement.textContent).not.toContain('请求低风险修复');
    await page.createRepair();
    expect(apiStub.createRepair).not.toHaveBeenCalled();
  });

  it('审批待处理时仅显示授权审批操作', async () => {
    const approval = {
      id: 91,
      repairRequestId: REPAIR.id,
      riskCategory: 'logical_change' as const,
      policyVersion: 'repair-policy-v1',
      status: 'pending' as const,
      requestedBy: 5,
      decidedBy: null,
      decisionReason: null,
      expiresAt: '2030-01-01T01:00:00Z',
      createdAt: '2026-08-26T00:02:00Z',
      updatedAt: '2026-08-26T00:02:00Z',
    };
    const page = fixture.componentInstance;
    page.repairs.set([{ ...REPAIR, approval }]);
    await fixture.whenStable();
    expect(fixture.nativeElement.textContent).not.toContain('批准');
    permissionState.set(new Set(['devrail:repair:approve']));
    await fixture.whenStable();
    expect(fixture.nativeElement.textContent).toContain('批准');
    expect(fixture.nativeElement.textContent).toContain('拒绝');
  });

  it('熔断和终态请求不显示可继续执行操作', async () => {
    const page = fixture.componentInstance;
    page.repairs.set([
      { ...REPAIR, status: 'handed_off', handoffReason: 'hook_failure_circuit_open' },
      { ...REPAIR, id: 42, status: 'succeeded', handoffReason: null },
    ]);
    permissionState.set(
      new Set(['devrail:repair:create', 'devrail:repair:cancel', 'devrail:repair:handoff']),
    );
    await fixture.whenStable();
    expect(fixture.nativeElement.textContent).not.toContain('请求低风险修复');
    expect(fixture.nativeElement.textContent).not.toContain('取消请求');
    expect(fixture.nativeElement.textContent).not.toContain('转人工处理');
  });

  it('修复空态使用中文，并阻止重复创建', async () => {
    permissionState.set(new Set(['devrail:repair:create']));
    const page = fixture.componentInstance;
    page.task.set({ ...TASK, status: 'failed' });
    page.runs.set([{ ...SOURCE_RUN, status: 'failed' }]);
    page.repairs.set([]);
    await fixture.whenStable();
    expect(fixture.nativeElement.textContent).toContain('暂无受控修复记录');
    const first = page.createRepair();
    const second = page.createRepair();
    await Promise.all([first, second]);
    expect(apiStub.createRepair).toHaveBeenCalledTimes(1);
  });

  it('修复创建失败时显示中文错误', async () => {
    permissionState.set(new Set(['devrail:repair:create']));
    const page = fixture.componentInstance;
    page.task.set({ ...TASK, status: 'failed' });
    page.runs.set([{ ...SOURCE_RUN, status: 'failed' }]);
    (apiStub.createRepair as ReturnType<typeof vi.fn>).mockRejectedValueOnce(new Error('network'));
    const snackOpen = vi.spyOn((page as unknown as { snack: MatSnackBar }).snack, 'open');
    expect(page.canCreateRepair()).toBe(true);
    await page.createRepair();
    expect(apiStub.createRepair).toHaveBeenCalledTimes(1);
    expect(snackOpen).toHaveBeenCalledWith('提交受控修复请求失败', '关闭', {
      duration: 5000,
    });
  });

  it('过期或撤回的审批显示状态且不提供审批操作', async () => {
    permissionState.set(new Set(['devrail:repair:approve']));
    const page = fixture.componentInstance;
    page.repairs.set([
      {
        ...REPAIR,
        approval: {
          id: 92,
          repairRequestId: REPAIR.id,
          riskCategory: 'logical_change',
          policyVersion: 'repair-policy-v1',
          status: 'pending',
          requestedBy: 5,
          decidedBy: null,
          decisionReason: null,
          expiresAt: '2020-01-01T00:00:00Z',
          createdAt: '2020-01-01T00:00:00Z',
          updatedAt: '2020-01-01T00:00:00Z',
        },
      },
      {
        ...REPAIR,
        id: 42,
        approval: {
          id: 93,
          repairRequestId: 42,
          riskCategory: 'logical_change',
          policyVersion: 'repair-policy-v1',
          status: 'withdrawn',
          requestedBy: 5,
          decidedBy: 5,
          decisionReason: null,
          expiresAt: '2030-01-01T00:00:00Z',
          createdAt: '2026-08-26T00:00:00Z',
          updatedAt: '2026-08-26T00:01:00Z',
        },
      },
    ]);
    await fixture.whenStable();
    const text = fixture.nativeElement.textContent as string;
    expect(text).toContain('审批：已过期');
    expect(text).toContain('审批：已撤回');
    expect(text).not.toContain('批准');
    expect(text).not.toContain('拒绝');
  });

  it('仅将未过期审批提交给批准或拒绝接口', async () => {
    permissionState.set(new Set(['devrail:repair:approve']));
    const approval = {
      id: 94,
      repairRequestId: REPAIR.id,
      riskCategory: 'logical_change' as const,
      policyVersion: 'repair-policy-v1',
      status: 'pending' as const,
      requestedBy: 5,
      decidedBy: null,
      decisionReason: null,
      expiresAt: '2030-01-01T00:00:00Z',
      createdAt: '2026-08-26T00:00:00Z',
      updatedAt: '2026-08-26T00:00:00Z',
    };
    const page = fixture.componentInstance;
    await page.decideRepairApproval({ ...REPAIR, approval }, true);
    await page.decideRepairApproval({ ...REPAIR, approval }, false);
    expect(apiStub.approveRepair).toHaveBeenCalledWith(approval.id, {});
    expect(apiStub.rejectRepair).toHaveBeenCalledWith(approval.id, {
      reason: '人工审核未通过',
    });
  });

  it('仅允许申请人撤回审批，并支持已交接请求的人工重试', async () => {
    const page = fixture.componentInstance;
    const approval = {
      id: 95,
      repairRequestId: REPAIR.id,
      riskCategory: 'logical_change' as const,
      policyVersion: 'repair-policy-v1',
      status: 'pending' as const,
      requestedBy: 5,
      decidedBy: null,
      decisionReason: null,
      expiresAt: '2030-01-01T01:00:00Z',
      createdAt: '2026-08-26T00:02:00Z',
      updatedAt: '2026-08-26T00:02:00Z',
    };
    page.repairs.set([{ ...REPAIR, status: 'handed_off', approval }]);
    permissionState.set(new Set(['devrail:repair:approve', 'devrail:repair:handoff']));
    await page.withdrawRepairApproval(approval);
    expect(apiStub.withdrawRepairApproval).toHaveBeenCalledWith(95, {
      reason: '申请人主动撤回审批',
    });
    await page.retryRepair({ ...REPAIR, status: 'handed_off' });
    expect(apiStub.retryRepair).toHaveBeenCalledWith(
      REPAIR.id,
      expect.objectContaining({ riskCategory: 'low_risk' }),
    );
  });

  it('未授权和人工交接状态不触发 repair 创建或管理请求', async () => {
    const page = fixture.componentInstance;
    page.task.set({ ...TASK, status: 'repair_handoff' });
    page.runs.set([{ ...SOURCE_RUN, status: 'failed' }]);
    await fixture.whenStable();
    expect(fixture.nativeElement.textContent).not.toContain('请求低风险修复');
    await page.createRepair();
    await page.cancelRepair(REPAIR);
    await page.handoffRepair(REPAIR);
    expect(apiStub.createRepair).not.toHaveBeenCalled();
    expect(apiStub.cancelRepair).not.toHaveBeenCalled();
    expect(apiStub.handoffRepair).not.toHaveBeenCalled();
  });

  it('展示人工交接原因并按权限执行取消和交接', async () => {
    permissionState.set(new Set(['devrail:repair:cancel', 'devrail:repair:handoff']));
    const page = fixture.componentInstance;
    const handedOff = { ...REPAIR, handoffReason: 'hook_failure_circuit_open' };
    page.repairs.set([handedOff]);
    await fixture.whenStable();
    expect(fixture.nativeElement.textContent).toContain('交接原因：Hook 失败熔断已打开');
    await page.cancelRepair(REPAIR);
    expect(apiStub.cancelRepair).toHaveBeenCalledWith(REPAIR.id);
    await page.handoffRepair(REPAIR);
    expect(apiStub.handoffRepair).toHaveBeenCalledWith(
      REPAIR.id,
      expect.objectContaining({ reasonCode: 'manual_handoff' }),
    );
  });

  it('终态 repair 和过期审批不会触发写操作', async () => {
    permissionState.set(
      new Set(['devrail:repair:cancel', 'devrail:repair:handoff', 'devrail:repair:approve']),
    );
    const page = fixture.componentInstance;
    const expiredApproval = {
      id: 92,
      repairRequestId: REPAIR.id,
      riskCategory: 'logical_change' as const,
      policyVersion: 'repair-policy-v1',
      status: 'pending' as const,
      requestedBy: 5,
      decidedBy: null,
      decisionReason: null,
      expiresAt: '2020-01-01T00:00:00Z',
      createdAt: '2020-01-01T00:00:00Z',
      updatedAt: '2020-01-01T00:00:00Z',
    };
    const terminal = { ...REPAIR, status: 'succeeded' as const, approval: expiredApproval };
    await page.cancelRepair(terminal);
    await page.handoffRepair(terminal);
    await page.decideRepairApproval(terminal, true);
    expect(apiStub.cancelRepair).not.toHaveBeenCalled();
    expect(apiStub.handoffRepair).not.toHaveBeenCalled();
    expect(apiStub.approveRepair).not.toHaveBeenCalled();
  });

  it('提交追加执行后恢复输入框焦点', async () => {
    permissionState.set(new Set(['devrail:continuation:create']));
    const page = fixture.componentInstance;
    page.task.set({ ...TASK, status: 'succeeded' });
    page.runs.set([SOURCE_RUN]);
    await fixture.whenStable();
    const form = fixture.nativeElement.querySelector('.continuation-form') as HTMLFormElement;
    page.continuationInput.set('请补充回归验证');
    await page.createContinuation(form);
    await new Promise((resolve) => window.setTimeout(resolve, 0));
    expect(document.activeElement).toBe(form.querySelector('textarea'));
  });

  it('按 UTF-8 字节限制追加上下文并提供可访问计数', async () => {
    permissionState.set(new Set(['devrail:continuation:create']));
    const page = fixture.componentInstance;
    page.task.set({
      ...TASK,
      status: 'succeeded',
      continuationPolicy: { ...TASK.continuationPolicy, max_context_bytes: 8 },
    });
    page.runs.set([SOURCE_RUN]);
    page.continuationInput.set('继续修复');
    await fixture.whenStable();
    const count = fixture.nativeElement.querySelector('#continuation-input-count') as HTMLElement;
    const submit = fixture.nativeElement.querySelector(
      '.continuation-form button',
    ) as HTMLButtonElement;
    expect(page.continuationBytes()).toBe(12);
    expect(count.textContent).toContain('12 / 8 字节');
    expect(count.getAttribute('aria-live')).toBe('polite');
    expect(submit.disabled).toBe(true);
  });
});
