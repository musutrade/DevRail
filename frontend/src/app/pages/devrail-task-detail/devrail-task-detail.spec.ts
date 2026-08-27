import { provideZonelessChangeDetection, signal } from '@angular/core';
import { ComponentFixture, TestBed } from '@angular/core/testing';
import { MatSnackBar } from '@angular/material/snack-bar';
import { ActivatedRoute, provideRouter } from '@angular/router';
import { vi } from 'vitest';
import { AuthService } from '../../core/auth.service';
import { DevRailApiService } from '../../features/devrail/data-access/devrail-api.service';
import type { DevRailTaskResponse } from '../../generated/api/models';
import { DevRailTaskDetailPage } from './devrail-task-detail';
import type { DevRailContinuationResponse, DevRailRunResponse } from '../../generated/api/models';

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

describe('DevRailTaskDetailPage', () => {
  let fixture: ComponentFixture<DevRailTaskDetailPage>;
  let apiStub: Partial<DevRailApiService>;
  const permissionState = signal<ReadonlySet<string>>(new Set());

  beforeEach(async () => {
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
          useValue: { hasPermission: (code: string) => permissionState().has(code) },
        },
        { provide: MatSnackBar, useValue: { open: vi.fn() } },
      ],
    }).compileComponents();

    fixture = TestBed.createComponent(DevRailTaskDetailPage);
    await vi.waitFor(() => expect(fixture.componentInstance.loading()).toBe(false));
    await fixture.whenStable();
  });

  afterEach(() => permissionState.set(new Set()));

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
