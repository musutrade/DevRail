import {
  ChangeDetectionStrategy,
  Component,
  ElementRef,
  OnDestroy,
  OnInit,
  computed,
  inject,
  signal,
  viewChild,
} from '@angular/core';
import { DatePipe } from '@angular/common';
import { ActivatedRoute, RouterLink } from '@angular/router';
import { MatIconModule } from '@angular/material/icon';
import { MatProgressSpinnerModule } from '@angular/material/progress-spinner';
import { MatSnackBar, MatSnackBarModule } from '@angular/material/snack-bar';
import { apiErrorMessage } from '../../core/api-error';
import { AuthService } from '../../core/auth.service';
import { DevRailApiService } from '../../features/devrail/data-access/devrail-api.service';
import { DEVRAIL_PERMISSIONS } from '../../features/devrail/devrail.permissions';
import type { DevRailTask } from '../../features/devrail/models/devrail.model';
import type {
  DevRailEnvironmentResponse,
  DevRailRepositoryResponse,
  DevRailTaskDependencyInput,
  DevRailTaskResponse,
  DevRailTaskWorkspaceResponse,
  DevRailContinuationResponse,
  DevRailRepairResponse,
  DevRailRunResponse,
} from '../../generated/api/models';
import type { DevRailTaskCommentResponse } from '../../generated/api/models';

@Component({
  selector: 'app-devrail-task-detail',
  imports: [DatePipe, MatIconModule, MatProgressSpinnerModule, MatSnackBarModule, RouterLink],
  templateUrl: './devrail-task-detail.html',
  styleUrl: './devrail-task-detail.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class DevRailTaskDetailPage implements OnDestroy, OnInit {
  readonly task = signal<DevRailTask | null>(null);
  readonly loading = signal(true);
  readonly busy = signal(false);
  readonly repositories = signal<DevRailRepositoryResponse[]>([]);
  readonly environments = signal<DevRailEnvironmentResponse[]>([]);
  readonly comments = signal<DevRailTaskCommentResponse[]>([]);
  readonly taskEvents = signal<import('../../generated/api/models').DevRailTaskEventResponse[]>([]);
  readonly workspace = signal<DevRailTaskWorkspaceResponse | null>(null);
  readonly continuations = signal<DevRailContinuationResponse[]>([]);
  readonly repairs = signal<DevRailRepairResponse[]>([]);
  readonly runs = signal<DevRailRunResponse[]>([]);
  readonly continuationInput = signal('');
  readonly continuationBusy = signal(false);
  readonly repairBusy = signal(false);
  readonly continuationByteLimit = computed(
    () => this.task()?.continuationPolicy.max_context_bytes ?? 16_384,
  );
  readonly continuationBytes = computed(
    () => new TextEncoder().encode(this.continuationInput()).length,
  );
  readonly continuationInputValid = computed(
    () =>
      this.continuationInput().trim().length > 0 &&
      this.continuationBytes() <= this.continuationByteLimit(),
  );
  readonly dependencyDraft = signal<DevRailTaskDependencyInput[]>([]);
  readonly dependencyCandidates = signal<DevRailTaskResponse[]>([]);
  readonly dependencyCandidateId = signal<number | null>(null);
  readonly dependencyBusy = signal(false);
  readonly canEditDependencies = computed(
    () =>
      this.auth.hasPermission(DEVRAIL_PERMISSIONS.taskDependencyWrite) &&
      ['draft', 'queued'].includes(this.task()?.status ?? ''),
  );
  readonly canManageWorkspace = computed(() =>
    this.auth.hasPermission(DEVRAIL_PERMISSIONS.workspaceWrite),
  );
  readonly continuationSourceRun = computed(
    () =>
      this.runs().find((run) => ['completed', 'failed'].includes(run.status) && !!run.turnId) ??
      null,
  );
  readonly repairSourceRun = computed(
    () => this.runs().find((run) => run.status === 'failed' && run.runKind !== 'repair') ?? null,
  );
  readonly canCreateContinuation = computed(
    () =>
      this.auth.hasPermission(DEVRAIL_PERMISSIONS.continuationCreate) &&
      this.task()?.continuationCapabilities.canCreate === true &&
      this.task()?.continuationPolicy.enabled === true &&
      !!this.continuationSourceRun() &&
      ['succeeded', 'failed'].includes(this.task()?.status ?? ''),
  );
  readonly canCancelContinuation = computed(
    () =>
      this.auth.hasPermission(DEVRAIL_PERMISSIONS.continuationCancel) &&
      this.task()?.continuationCapabilities.canCancel === true,
  );
  readonly canCreateRepair = computed(
    () =>
      this.auth.hasPermission(DEVRAIL_PERMISSIONS.repairCreate) &&
      !!this.repairSourceRun() &&
      this.task()?.repairPolicy.enabled === true &&
      this.task()?.status === 'failed',
  );
  readonly canCancelRepair = computed(() =>
    this.auth.hasPermission(DEVRAIL_PERMISSIONS.repairCancel),
  );
  readonly canApproveRepair = computed(() =>
    this.auth.hasPermission(DEVRAIL_PERMISSIONS.repairApprove),
  );
  readonly canHandoffRepair = computed(() =>
    this.auth.hasPermission(DEVRAIL_PERMISSIONS.repairHandoff),
  );
  readonly editingCommentId = signal<number | null>(null);
  private readonly route = inject(ActivatedRoute);
  private readonly auth = inject(AuthService);
  private readonly api = inject(DevRailApiService);
  private readonly snack = inject(MatSnackBar);
  private readonly repairHeading = viewChild<ElementRef<HTMLElement>>('repairHeading');
  projectId = 0;
  private taskId = 0;
  private eventSource?: EventSource;

  ngOnInit(): void {
    this.projectId = Number(this.route.snapshot.paramMap.get('projectId'));
    this.taskId = Number(this.route.snapshot.paramMap.get('taskId'));
    void this.load();
  }

  ngOnDestroy(): void {
    this.eventSource?.close();
  }

  async save(form: HTMLFormElement): Promise<void> {
    const data = new FormData(form);
    this.busy.set(true);
    try {
      const updated = await this.api.updateTask(this.projectId, this.taskId, {
        title: String(data.get('title') || ''),
        goal: String(data.get('goal') || ''),
        priority: String(data.get('priority') || 'normal'),
        status: String(data.get('status') || 'draft'),
        background: String(data.get('background') || '') || null,
        acceptanceCriteria: String(data.get('acceptanceCriteria') || '') || null,
        constraints: String(data.get('constraints') || '') || null,
        repositoryId: data.get('repositoryId') ? Number(data.get('repositoryId')) : null,
        environmentId: data.get('environmentId') ? Number(data.get('environmentId')) : null,
      });
      this.task.set(updated);
      this.snack.open('任务已保存', '关闭', { duration: 2500 });
    } catch (error) {
      this.snack.open(apiErrorMessage(error, '任务保存失败'), '关闭', { duration: 5000 });
    } finally {
      this.busy.set(false);
    }
  }

  async rebuildWorkspace(): Promise<void> {
    if (this.busy() || !this.canManageWorkspace()) return;
    this.busy.set(true);
    try {
      this.workspace.set(await this.api.rebuildTaskWorkspace(this.taskId));
      this.snack.open('任务工作区已准备', '关闭', { duration: 2500 });
    } catch (error) {
      this.snack.open(apiErrorMessage(error, '准备任务工作区失败'), '关闭', { duration: 5000 });
    } finally {
      this.busy.set(false);
    }
  }

  continuationStatusLabel(status: string): string {
    return (
      {
        pending: '待派发',
        claimed: '已领取',
        dispatched: '已派发',
        completed: '已完成',
        cancelled: '已取消',
        rejected: '已拒绝',
      }[status] ?? status
    );
  }

  continuationTriggerLabel(trigger: string): string {
    return (
      {
        user_context: '用户追加',
        quality_gate: '质量门禁',
        review_changes: '审查修改',
      }[trigger] ?? trigger
    );
  }

  onContinuationInput(event: Event): void {
    this.continuationInput.set((event.target as HTMLTextAreaElement).value);
  }

  async createContinuation(form: HTMLFormElement): Promise<void> {
    const source = this.continuationSourceRun();
    const input = this.continuationInput().trim();
    if (
      !source ||
      !this.continuationInputValid() ||
      this.continuationBusy() ||
      !this.canCreateContinuation()
    )
      return;
    this.continuationBusy.set(true);
    try {
      const created = await this.api.createContinuation(source.id, {
        input,
        idempotencyKey: `ui-continuation-${source.id}-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`,
      });
      this.continuations.update((items) => [created, ...items]);
      this.continuationInput.set('');
      form.reset();
      this.snack.open('追加执行请求已提交', '关闭', { duration: 3000 });
      window.setTimeout(() => form.querySelector<HTMLTextAreaElement>('textarea')?.focus(), 0);
    } catch (error) {
      this.snack.open(apiErrorMessage(error, '提交追加执行请求失败'), '关闭', { duration: 5000 });
    } finally {
      this.continuationBusy.set(false);
    }
  }

  async cancelContinuation(item: DevRailContinuationResponse): Promise<void> {
    if (this.continuationBusy() || !['pending', 'claimed'].includes(item.status)) return;
    this.continuationBusy.set(true);
    try {
      const cancelled = await this.api.cancelContinuation(item.id);
      this.continuations.update((items) =>
        items.map((current) => (current.id === cancelled.id ? cancelled : current)),
      );
      this.snack.open('追加执行请求已取消', '关闭', { duration: 2500 });
    } catch (error) {
      this.snack.open(apiErrorMessage(error, '取消追加执行请求失败'), '关闭', { duration: 5000 });
    } finally {
      this.continuationBusy.set(false);
    }
  }

  repairStatusLabel(status: string): string {
    return (
      {
        pending: '待派发',
        claimed: '已领取',
        dispatched: '已派发',
        running: '修复运行中',
        succeeded: '修复成功',
        failed: '修复失败',
        cancelled: '已取消',
        handed_off: '等待人工处理',
        rejected: '已拒绝',
      }[status] ?? status
    );
  }

  repairRiskLabel(category: string): string {
    return (
      {
        low_risk: '低风险',
        logical_change: '逻辑修改',
        dependency_change: '依赖修改',
        remote_write: '远端写入',
        security_change: '安全策略修改',
        forbidden: '禁止自动处理',
      }[category] ?? category
    );
  }

  repairHandoffReasonLabel(reason: string): string {
    return (
      {
        policy_disabled: '当前任务策略未启用自动修复',
        approval_required: '修复操作需要审批',
        approval_expired: '修复审批已过期或撤回',
        approval_rejected: '修复审批未通过',
        budget_exceeded: '修复次数或成本已达到上限',
        hook_failure_circuit_open: 'Hook 失败熔断已打开',
        gate_failed: '受影响门禁未通过',
        manual_handoff: '已由人工转交处理',
      }[reason] ?? '修复自动化无法继续执行'
    );
  }

  repairApprovalStatusLabel(approval: DevRailRepairResponse['approval']): string {
    if (!approval) return '无审批';
    if (approval.status === 'pending' && new Date(approval.expiresAt).getTime() <= Date.now()) {
      return '已过期';
    }
    return (
      {
        pending: '待审批',
        approved: '已批准',
        rejected: '已拒绝',
        expired: '已过期',
        withdrawn: '已撤回',
      }[approval.status] ?? approval.status
    );
  }

  isRepairApprovalActionable(approval: NonNullable<DevRailRepairResponse['approval']>): boolean {
    return approval.status === 'pending' && new Date(approval.expiresAt).getTime() > Date.now();
  }

  canWithdrawRepairApproval(approval: NonNullable<DevRailRepairResponse['approval']>): boolean {
    const currentUser = this.auth.currentUser?.();
    return (
      this.canApproveRepair() &&
      this.isRepairApprovalActionable(approval) &&
      currentUser?.id === approval.requestedBy
    );
  }

  async createRepair(): Promise<void> {
    const source = this.repairSourceRun();
    if (!source || this.repairBusy() || !this.canCreateRepair()) return;
    this.repairBusy.set(true);
    try {
      const repair = await this.api.createRepair(source.id, {
        idempotencyKey: `ui-repair-${source.id}-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`,
        riskCategory: 'low_risk',
      });
      this.repairs.update((items) => [repair, ...items.filter((item) => item.id !== repair.id)]);
      this.snack.open('低风险修复请求已提交', '关闭', { duration: 3000 });
      await this.refreshRepairData();
      this.restoreRepairFocus();
    } catch (error) {
      this.snack.open(apiErrorMessage(error, '提交受控修复请求失败'), '关闭', { duration: 5000 });
    } finally {
      this.repairBusy.set(false);
    }
  }

  async cancelRepair(item: DevRailRepairResponse): Promise<void> {
    if (
      this.repairBusy() ||
      !this.canCancelRepair() ||
      !['pending', 'claimed'].includes(item.status)
    )
      return;
    this.repairBusy.set(true);
    try {
      const updated = await this.api.cancelRepair(item.id);
      this.replaceRepair(updated);
      this.snack.open('受控修复请求已取消', '关闭', { duration: 2500 });
      await this.refreshRepairData();
      this.restoreRepairFocus();
    } catch (error) {
      this.snack.open(apiErrorMessage(error, '取消受控修复请求失败'), '关闭', { duration: 5000 });
    } finally {
      this.repairBusy.set(false);
    }
  }

  async handoffRepair(item: DevRailRepairResponse): Promise<void> {
    if (
      this.repairBusy() ||
      !this.canHandoffRepair() ||
      !['pending', 'claimed', 'dispatched', 'running'].includes(item.status)
    )
      return;
    this.repairBusy.set(true);
    try {
      const updated = await this.api.handoffRepair(item.id, {
        reasonCode: 'manual_handoff',
        recommendation: '请由授权人员复核失败诊断、策略和质量门禁结果。',
      });
      this.replaceRepair(updated);
      this.snack.open('受控修复已转人工处理', '关闭', { duration: 2500 });
      await this.refreshRepairData();
      this.restoreRepairFocus();
    } catch (error) {
      this.snack.open(apiErrorMessage(error, '转人工处理失败'), '关闭', { duration: 5000 });
    } finally {
      this.repairBusy.set(false);
    }
  }

  async retryRepair(item: DevRailRepairResponse): Promise<void> {
    if (this.repairBusy() || !this.canHandoffRepair() || item.status !== 'handed_off') return;
    this.repairBusy.set(true);
    try {
      const updated = await this.api.retryRepair(item.id, {
        idempotencyKey: `ui-repair-retry-${item.id}-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`,
        riskCategory: item.riskCategory,
      });
      this.repairs.update((items) => [
        updated,
        ...items.filter((current) => current.id !== updated.id),
      ]);
      this.snack.open('人工重试请求已提交', '关闭', { duration: 2500 });
      await this.refreshRepairData();
      this.restoreRepairFocus();
    } catch (error) {
      this.snack.open(apiErrorMessage(error, '提交人工重试失败'), '关闭', { duration: 5000 });
    } finally {
      this.repairBusy.set(false);
    }
  }

  async withdrawRepairApproval(
    approval: NonNullable<DevRailRepairResponse['approval']>,
  ): Promise<void> {
    if (this.repairBusy() || !this.canWithdrawRepairApproval(approval)) return;
    this.repairBusy.set(true);
    try {
      await this.api.withdrawRepairApproval(approval.id, { reason: '申请人主动撤回审批' });
      await this.refreshRepairData();
      this.restoreRepairFocus();
      this.snack.open('修复审批已撤回', '关闭', { duration: 2500 });
    } catch (error) {
      this.snack.open(apiErrorMessage(error, '撤回修复审批失败'), '关闭', { duration: 5000 });
    } finally {
      this.repairBusy.set(false);
    }
  }

  async decideRepairApproval(item: DevRailRepairResponse, approved: boolean): Promise<void> {
    const approval = item.approval;
    if (
      !approval ||
      !this.isRepairApprovalActionable(approval) ||
      this.repairBusy() ||
      !this.canApproveRepair()
    )
      return;
    this.repairBusy.set(true);
    try {
      if (approved) {
        await this.api.approveRepair(approval.id, {});
      } else {
        await this.api.rejectRepair(approval.id, { reason: '人工审核未通过' });
      }
      await this.refreshRepairData();
      this.restoreRepairFocus();
      this.snack.open(approved ? '修复审批已通过' : '修复审批已拒绝', '关闭', { duration: 2500 });
    } catch (error) {
      this.snack.open(apiErrorMessage(error, '处理修复审批失败'), '关闭', { duration: 5000 });
    } finally {
      this.repairBusy.set(false);
    }
  }

  private restoreRepairFocus(): void {
    queueMicrotask(() => this.repairHeading()?.nativeElement.focus());
  }

  async addComment(form: HTMLFormElement): Promise<void> {
    const body = new FormData(form).get('body');
    if (typeof body !== 'string' || !body.trim()) return;
    this.busy.set(true);
    try {
      const comment = await this.api.createTaskComment(this.taskId, { body: body.trim() });
      this.comments.update((items) => [...items, comment]);
      form.reset();
    } catch (error) {
      this.snack.open(apiErrorMessage(error, '评论发布失败'), '关闭', { duration: 5000 });
    } finally {
      this.busy.set(false);
    }
  }
  startEdit(comment: DevRailTaskCommentResponse): void {
    this.editingCommentId.set(comment.id);
  }
  cancelEdit(): void {
    this.editingCommentId.set(null);
  }
  async saveComment(comment: DevRailTaskCommentResponse, form: HTMLFormElement): Promise<void> {
    const body = new FormData(form).get('body');
    if (typeof body !== 'string' || !body.trim()) return;
    this.busy.set(true);
    try {
      const updated = await this.api.updateTaskComment(comment.id, { body: body.trim() });
      this.comments.update((items) =>
        items.map((item) => (item.id === updated.id ? updated : item)),
      );
      this.editingCommentId.set(null);
    } catch (error) {
      this.snack.open(apiErrorMessage(error, '评论保存失败'), '关闭', { duration: 5000 });
    } finally {
      this.busy.set(false);
    }
  }
  async removeComment(comment: DevRailTaskCommentResponse): Promise<void> {
    if (!window.confirm('确定删除这条评论吗？')) return;
    this.busy.set(true);
    try {
      await this.api.deleteTaskComment(comment.id);
      this.comments.update((items) =>
        items.map((item) =>
          item.id === comment.id
            ? { ...item, body: '[评论已删除]', deleted: true, mentions: [] }
            : item,
        ),
      );
    } catch (error) {
      this.snack.open(apiErrorMessage(error, '评论删除失败'), '关闭', { duration: 5000 });
    } finally {
      this.busy.set(false);
    }
  }

  addDependency(): void {
    const prerequisiteTaskId = this.dependencyCandidateId();
    const taskId = this.taskId;
    if (
      prerequisiteTaskId === null ||
      prerequisiteTaskId === taskId ||
      this.dependencyDraft().some((item) => item.prerequisiteTaskId === prerequisiteTaskId)
    ) {
      this.snack.open('请选择未添加且不是当前任务的前置任务', '关闭', { duration: 3000 });
      return;
    }
    this.dependencyDraft.update((items) => [
      ...items,
      {
        prerequisiteTaskId,
        failureAction: 'wait',
        cancelledAction: 'wait',
        timeoutAction: 'wait',
      },
    ]);
    this.dependencyCandidateId.set(null);
  }

  removeDependency(prerequisiteTaskId: number): void {
    this.dependencyDraft.update((items) =>
      items.filter((item) => item.prerequisiteTaskId !== prerequisiteTaskId),
    );
  }

  async saveDependencies(): Promise<void> {
    const current = this.task();
    if (!current || !this.canEditDependencies()) return;
    this.dependencyBusy.set(true);
    try {
      const relations = await this.api.replaceTaskDependencies(this.projectId, this.taskId, {
        revision: current.revision,
        idempotencyKey: `ui-${this.taskId}-${current.revision}-${Date.now()}`,
        dependencies: this.dependencyDraft(),
      });
      this.task.update((task) =>
        task
          ? {
              ...task,
              revision: relations.revision,
              prerequisites: relations.prerequisites,
              dependents: relations.dependents,
              blockedReason: relations.blockedReason,
            }
          : task,
      );
      this.snack.open('任务依赖已保存', '关闭', { duration: 2500 });
    } catch (error) {
      this.snack.open(apiErrorMessage(error, '任务依赖保存失败，可能存在版本或环冲突'), '关闭', {
        duration: 5000,
      });
    } finally {
      this.dependencyBusy.set(false);
    }
  }

  private async load(): Promise<void> {
    try {
      const workspacePromise =
        typeof this.api.getTaskWorkspace === 'function'
          ? this.api.getTaskWorkspace(this.taskId).catch(() => null)
          : Promise.resolve(null);
      const runsPromise =
        typeof this.api.listRuns === 'function'
          ? this.api
              .listRuns(this.taskId, 1, 100)
              .catch(() => ({ items: [], total: 0, page: 1, pageSize: 100 }))
          : Promise.resolve({ items: [], total: 0, page: 1, pageSize: 100 });
      const continuationsPromise =
        typeof this.api.listContinuations === 'function'
          ? this.api
              .listContinuations(this.taskId, undefined, 1, 100)
              .catch(() => ({ items: [], total: 0, page: 1, pageSize: 100 }))
          : Promise.resolve({ items: [], total: 0, page: 1, pageSize: 100 });
      const repairsPromise =
        typeof this.api.listRepairs === 'function'
          ? this.api
              .listRepairs(this.taskId, undefined, 1, 100)
              .catch(() => ({ items: [], total: 0, page: 1, pageSize: 100 }))
          : Promise.resolve({ items: [], total: 0, page: 1, pageSize: 100 });
      const [
        task,
        repositories,
        environments,
        comments,
        events,
        candidates,
        workspace,
        runs,
        continuations,
        repairs,
      ] = await Promise.all([
        this.api.getTask(this.projectId, this.taskId),
        this.api.listRepositories(this.projectId),
        this.api.listEnvironments(this.projectId),
        this.api.listTaskComments(this.taskId),
        this.api.listTaskEvents(this.taskId),
        this.api.listTasks(this.projectId, 1, 100),
        workspacePromise,
        runsPromise,
        continuationsPromise,
        repairsPromise,
      ]);
      this.task.set(task);
      this.repositories.set(repositories.items);
      this.environments.set(environments.items);
      this.comments.set(comments.items);
      this.taskEvents.set(events.items);
      this.workspace.set(workspace);
      this.runs.set(runs.items);
      this.continuations.set(continuations.items);
      this.repairs.set(repairs.items);
      this.dependencyDraft.set(
        task.prerequisites.map((dependency) => ({
          prerequisiteTaskId: dependency.prerequisiteTaskId,
          failureAction: dependency.failureAction as 'wait' | 'skip' | 'fail',
          cancelledAction: dependency.cancelledAction as 'wait' | 'skip' | 'fail',
          timeoutAction: dependency.timeoutAction as 'wait' | 'skip' | 'fail',
        })),
      );
      this.dependencyCandidates.set(
        candidates.items.filter((candidate) => candidate.id !== task.id),
      );
      this.startEventStream();
    } catch (error) {
      this.snack.open(apiErrorMessage(error, '任务加载失败'), '关闭', { duration: 5000 });
    } finally {
      this.loading.set(false);
    }
  }

  private startEventStream(): void {
    if (typeof EventSource === 'undefined') return;
    this.eventSource?.close();
    const source = new EventSource(`/api/v1/tasks/${this.taskId}/events/stream`);
    const refresh = () => {
      void this.api
        .getTask(this.projectId, this.taskId)
        .then((task) => this.task.set(task))
        .catch(() => undefined);
    };
    source.onmessage = refresh;
    for (const eventType of [
      'task.dependencies.changed',
      'task.dependency.propagated',
      'task.followup.created',
      'task.created.from_followup',
      'continuation.created',
      'continuation.claimed',
      'continuation.dispatched',
      'continuation.cancelled',
      'continuation.rejected',
      'continuation.completed',
      'repair.created',
      'repair.dispatched',
      'repair.completed',
      'repair.cancelled',
      'repair.handed_off',
    ]) {
      source.addEventListener(eventType, () => {
        refresh();
        if (typeof this.api.listRuns === 'function') {
          void this.api
            .listRuns(this.taskId, 1, 100)
            .then((page) => this.runs.set(page.items))
            .catch(() => undefined);
        }
        if (typeof this.api.listContinuations === 'function') {
          void this.api
            .listContinuations(this.taskId, undefined, 1, 100)
            .then((page) => this.continuations.set(page.items))
            .catch(() => undefined);
        }
        void this.refreshRepairData();
      });
    }
    source.onerror = () => {
      source.close();
    };
    this.eventSource = source;
  }

  private replaceRepair(updated: DevRailRepairResponse): void {
    this.repairs.update((items) => items.map((item) => (item.id === updated.id ? updated : item)));
  }

  private async refreshRepairData(): Promise<void> {
    const [repairs, task, runs] = await Promise.all([
      this.api.listRepairs(this.taskId, undefined, 1, 100),
      this.api.getTask(this.projectId, this.taskId),
      this.api.listRuns(this.taskId, 1, 100),
    ]);
    this.repairs.set(repairs.items);
    this.task.set(task);
    this.runs.set(runs.items);
  }
}
