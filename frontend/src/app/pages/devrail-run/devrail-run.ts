import { DatePipe } from '@angular/common';
import {
  ChangeDetectionStrategy,
  Component,
  OnDestroy,
  OnInit,
  computed,
  inject,
  signal,
} from '@angular/core';
import { ActivatedRoute, RouterLink } from '@angular/router';
import { MatIconModule } from '@angular/material/icon';
import { MatProgressSpinnerModule } from '@angular/material/progress-spinner';
import { MatSnackBar, MatSnackBarModule } from '@angular/material/snack-bar';
import { apiErrorMessage } from '../../core/api-error';
import { API_BASE_URL } from '../../core/runtime-config';
import { safeDownloadFileName, safeDownloadUrl } from '../../core/safe-navigation';
import { AuthService } from '../../core/auth.service';
import { DevRailApiService } from '../../features/devrail/data-access/devrail-api.service';
import { DEVRAIL_PERMISSIONS } from '../../features/devrail/devrail.permissions';
import type {
  DevRailChangeFileResponse,
  DevRailQualityGateResponse,
  DevRailRunEventResponse,
  DevRailRunResponse,
  DevRailReviewResponse,
  DevRailReviewCommentResponse,
  DevRailExternalReviewCommentResponse,
  DevRailTaskWorkspaceResponse,
  DevRailContinuationResponse,
  DevRailRepairResponse,
  DevRailArtifactResponse,
} from '../../generated/api/models';

@Component({
  selector: 'app-devrail-run',
  imports: [DatePipe, MatIconModule, MatProgressSpinnerModule, MatSnackBarModule, RouterLink],
  templateUrl: './devrail-run.html',
  styleUrl: './devrail-run.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class DevRailRunPage implements OnInit, OnDestroy {
  readonly run = signal<DevRailRunResponse | null>(null);
  readonly events = signal<DevRailRunEventResponse[]>([]);
  readonly changes = signal<DevRailChangeFileResponse[]>([]);
  readonly qualityGates = signal<DevRailQualityGateResponse[]>([]);
  readonly artifacts = signal<DevRailArtifactResponse[]>([]);
  readonly reviews = signal<DevRailReviewResponse[]>([]);
  readonly reviewerUserId = signal('');
  readonly reviewSummary = signal('');
  readonly reviewComments = signal<DevRailReviewCommentResponse[]>([]);
  readonly selectedReviewId = signal<number | null>(null);
  readonly externalReviewComments = signal<DevRailExternalReviewCommentResponse[]>([]);
  readonly workspace = signal<DevRailTaskWorkspaceResponse | null>(null);
  readonly continuations = signal<DevRailContinuationResponse[]>([]);
  readonly repair = signal<DevRailRepairResponse | null>(null);
  readonly externalProjectId = signal('');
  readonly externalRepositoryId = signal('');
  readonly externalNumber = signal('');
  readonly commentFilePath = signal('');
  readonly commentBody = signal('');
  readonly loading = signal(true);
  readonly busy = signal(false);
  readonly error = signal<string | null>(null);
  readonly canExecute = computed(() => this.auth.hasPermission(DEVRAIL_PERMISSIONS.runExecute));
  readonly canInterrupt = computed(() => this.auth.hasPermission(DEVRAIL_PERMISSIONS.runInterrupt));
  readonly canRetry = computed(() => this.auth.hasPermission(DEVRAIL_PERMISSIONS.runRetry));
  readonly canCancelContinuation = computed(() =>
    this.auth.hasPermission(DEVRAIL_PERMISSIONS.continuationCancel),
  );
  readonly canManageWorkspace = computed(() =>
    this.auth.hasPermission(DEVRAIL_PERMISSIONS.workspaceWrite),
  );
  private readonly route = inject(ActivatedRoute);
  private readonly auth = inject(AuthService);
  private readonly api = inject(DevRailApiService);
  private readonly snack = inject(MatSnackBar);
  private readonly apiBaseUrl = inject(API_BASE_URL);
  private eventSource?: EventSource;
  private reconnectTimer?: number;
  private destroyed = false;
  private runId = 0;
  private lastEventCursor = 0;

  ngOnInit(): void {
    this.runId = Number(this.route.snapshot.paramMap.get('id'));
    void this.load();
  }

  ngOnDestroy(): void {
    this.destroyed = true;
    if (this.reconnectTimer !== undefined) {
      window.clearTimeout(this.reconnectTimer);
      this.reconnectTimer = undefined;
    }
    this.eventSource?.close();
    this.eventSource = undefined;
  }

  async interrupt(): Promise<void> {
    if (!this.run() || this.busy()) return;
    this.busy.set(true);
    try {
      this.run.set(await this.api.interruptRun(this.runId));
      this.snack.open('运行已请求中断', '关闭', { duration: 2500 });
    } catch (error) {
      this.snack.open(apiErrorMessage(error, '中断运行失败'), '关闭', { duration: 5000 });
    } finally {
      this.busy.set(false);
    }
  }

  async retry(): Promise<void> {
    await this.performRetry(false);
  }

  async retryFromTurn(): Promise<void> {
    await this.performRetry(true);
  }

  async executeQualityGates(): Promise<void> {
    if (this.busy()) return;
    this.busy.set(true);
    try {
      this.qualityGates.set((await this.api.executeRunQualityGates(this.runId)).items);
      this.run.set(await this.api.getRun(this.runId));
      this.snack.open('质量门禁执行完成', '关闭', { duration: 2500 });
    } catch (error) {
      this.snack.open(apiErrorMessage(error, '质量门禁执行失败'), '关闭', { duration: 5000 });
    } finally {
      this.busy.set(false);
    }
  }

  async cleanupWorkspace(): Promise<void> {
    const current = this.workspace();
    if (!current || this.busy() || !this.canManageWorkspace()) return;
    this.busy.set(true);
    try {
      this.workspace.set(await this.api.cleanupWorkspace(current.id));
      this.snack.open('工作区清理完成', '关闭', { duration: 2500 });
    } catch (error) {
      this.snack.open(apiErrorMessage(error, '工作区清理失败'), '关闭', { duration: 5000 });
    } finally {
      this.busy.set(false);
    }
  }

  async downloadPatch(): Promise<void> {
    if (this.busy()) return;
    this.busy.set(true);
    try {
      const patch = await this.api.exportRunPatch(this.runId);
      const url = URL.createObjectURL(new Blob([patch.content], { type: 'text/x-diff' }));
      const link = document.createElement('a');
      link.href = url;
      link.download = safeDownloadFileName(patch.fileName) ?? `devrail-run-${this.runId}.patch`;
      link.click();
      URL.revokeObjectURL(url);
      this.snack.open('补丁已下载', '关闭', { duration: 2500 });
    } catch (error) {
      this.snack.open(apiErrorMessage(error, '导出补丁失败'), '关闭', { duration: 5000 });
    } finally {
      this.busy.set(false);
    }
  }

  downloadArtifact(artifact: DevRailArtifactResponse): void {
    if (this.busy()) return;
    const url = safeDownloadUrl(artifact.downloadUrl);
    const fileName = safeDownloadFileName(artifact.fileName);
    if (!url || !fileName) {
      this.snack.open('产物下载地址或文件名不安全', '关闭', { duration: 5000 });
      return;
    }
    const link = document.createElement('a');
    link.href = url;
    link.download = fileName;
    link.rel = 'noopener';
    link.click();
  }

  onReviewerUserIdInput(event: Event): void {
    this.reviewerUserId.set((event.target as HTMLInputElement).value);
  }

  onReviewSummaryInput(event: Event): void {
    this.reviewSummary.set((event.target as HTMLInputElement).value);
  }

  async createReview(): Promise<void> {
    const reviewerUserId = Number(this.reviewerUserId());
    if (!Number.isInteger(reviewerUserId) || reviewerUserId <= 0 || this.busy()) return;
    this.busy.set(true);
    try {
      await this.api.createReview({
        runId: this.runId,
        reviewerUserId,
        summary: this.reviewSummary().trim() || null,
      });
      this.reviewerUserId.set('');
      this.reviewSummary.set('');
      await this.loadReviews();
      this.snack.open('审查请求已创建', '关闭', { duration: 2500 });
    } catch (error) {
      this.snack.open(apiErrorMessage(error, '创建审查请求失败'), '关闭', { duration: 5000 });
    } finally {
      this.busy.set(false);
    }
  }

  async decideReview(
    review: DevRailReviewResponse,
    decision: 'approved' | 'rejected',
  ): Promise<void> {
    if (review.status !== 'pending' || this.busy()) return;
    this.busy.set(true);
    try {
      await this.api.decideReview(review.id, { decision });
      await this.loadReviews();
      this.snack.open(decision === 'approved' ? '审查已通过' : '审查已驳回', '关闭', {
        duration: 2500,
      });
    } catch (error) {
      this.snack.open(apiErrorMessage(error, '处理审查失败'), '关闭', { duration: 5000 });
    } finally {
      this.busy.set(false);
    }
  }

  onCommentFilePathInput(event: Event): void {
    this.commentFilePath.set((event.target as HTMLInputElement).value);
  }
  onCommentBodyInput(event: Event): void {
    this.commentBody.set((event.target as HTMLInputElement).value);
  }
  async selectReview(review: DevRailReviewResponse): Promise<void> {
    this.selectedReviewId.set(review.id);
    this.reviewComments.set(await this.api.listReviewComments(review.id));
    this.externalReviewComments.set(await this.api.listExternalReviewComments(review.id));
  }
  async syncExternalReviewComments(): Promise<void> {
    const reviewId = this.selectedReviewId();
    const projectId = Number(this.externalProjectId());
    const repositoryId = Number(this.externalRepositoryId());
    const number = Number(this.externalNumber());
    if (
      !reviewId ||
      ![projectId, repositoryId, number].every(
        (value) => Number.isSafeInteger(value) && value > 0,
      ) ||
      this.busy()
    )
      return;
    this.busy.set(true);
    try {
      this.externalReviewComments.set(
        await this.api.syncExternalReviewComments(reviewId, { projectId, repositoryId, number }),
      );
      this.snack.open('外部审查意见已同步', '关闭', { duration: 2500 });
    } catch (error) {
      this.snack.open(apiErrorMessage(error, '同步外部审查意见失败'), '关闭', { duration: 5000 });
    } finally {
      this.busy.set(false);
    }
  }
  async createReviewComment(): Promise<void> {
    const reviewId = this.selectedReviewId();
    if (!reviewId || !this.commentFilePath().trim() || !this.commentBody().trim() || this.busy())
      return;
    this.busy.set(true);
    try {
      const comment = await this.api.createReviewComment(reviewId, {
        filePath: this.commentFilePath().trim(),
        body: this.commentBody().trim(),
      });
      this.reviewComments.update((items) => [...items, comment]);
      this.commentFilePath.set('');
      this.commentBody.set('');
    } catch (error) {
      this.snack.open(apiErrorMessage(error, '添加审查意见失败'), '关闭', { duration: 5000 });
    } finally {
      this.busy.set(false);
    }
  }

  private async performRetry(resume: boolean): Promise<void> {
    const current = this.run();
    if (!current || this.busy()) return;
    this.busy.set(true);
    try {
      const next = await this.api.retryRun(this.runId, {
        idempotencyKey: `ui-retry-${Date.now()}`,
        input: null,
        ...(resume && current.turnId ? { resumeFromTurnId: current.turnId } : {}),
      });
      this.snack.open(`已创建新的运行 #${next.id}`, '关闭', { duration: 3000 });
      this.runId = next.id;
      this.lastEventCursor = 0;
      this.run.set(next);
      this.events.set([]);
      this.connectEvents();
    } catch (error) {
      this.snack.open(apiErrorMessage(error, '重试运行失败'), '关闭', { duration: 5000 });
    } finally {
      this.busy.set(false);
    }
  }

  eventPayload(event: DevRailRunEventResponse): string {
    return event.summary || JSON.stringify(event.payload);
  }

  statusLabel(status: string): string {
    return (
      {
        created: '已创建',
        starting: '启动中',
        active: '运行中',
        awaiting_approval: '等待审批',
        completed: '已完成',
        failed: '失败',
        cancelled: '已取消',
      }[status] ?? status
    );
  }

  actorLabel(actorType: string): string {
    return actorType === 'system' ? '系统调度器' : '用户';
  }

  runKindLabel(kind: string): string {
    return (
      {
        primary: '主运行',
        retry: '失败重试',
        continuation: '追加执行',
        follow_up: '后续任务运行',
        repair: '受控修复',
      }[kind] ?? kind
    );
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

  handoffStatusLabel(status: string | null | undefined): string {
    return (
      {
        available: '可用于追加执行',
        missing: '缺少可验证证据',
        invalid: '证据校验失败',
      }[status ?? ''] ?? '不可用'
    );
  }

  async cancelPendingContinuation(item: DevRailContinuationResponse): Promise<void> {
    if (this.busy() || !['pending', 'claimed'].includes(item.status)) return;
    this.busy.set(true);
    try {
      const cancelled = await this.api.cancelContinuation(item.id);
      this.continuations.update((items) =>
        items.map((current) => (current.id === cancelled.id ? cancelled : current)),
      );
      this.snack.open('追加执行请求已取消', '关闭', { duration: 2500 });
    } catch (error) {
      this.snack.open(apiErrorMessage(error, '取消追加执行请求失败'), '关闭', { duration: 5000 });
    } finally {
      this.busy.set(false);
    }
  }

  private async load(): Promise<void> {
    this.loading.set(true);
    this.error.set(null);
    try {
      const currentRun = await this.api.getRun(this.runId);
      this.run.set(currentRun);
      const repairPromise = currentRun.repairRequestId
        ? this.api.getRepair(currentRun.repairRequestId).catch(() => null)
        : this.api
            .listRepairs(undefined, this.runId, 1, 50)
            .then((page) => page.items[0] ?? null)
            .catch(() => null);
      const workspacePromise =
        typeof this.api.getRunWorkspace === 'function'
          ? this.api.getRunWorkspace(this.runId).catch(() => null)
          : Promise.resolve(null);
      const [page, changeset, gates, workspace, continuations, repair, artifacts] =
        await Promise.all([
          this.api.listRunEvents(this.runId),
          this.api.getRunChangeset(this.runId),
          this.api.getRunQualityGates(this.runId),
          workspacePromise,
          typeof this.api.listContinuations === 'function'
            ? this.api
                .listContinuations(undefined, this.runId, 1, 50)
                .catch(() => ({ items: [], total: 0, page: 1, pageSize: 50 }))
            : Promise.resolve({ items: [], total: 0, page: 1, pageSize: 50 }),
          repairPromise,
          typeof this.api.listArtifacts === 'function'
            ? this.api.listArtifacts(undefined, this.runId, 1, 100).catch(() => ({
                items: [],
                total: 0,
                page: 1,
                pageSize: 100,
              }))
            : Promise.resolve({ items: [], total: 0, page: 1, pageSize: 100 }),
        ]);
      this.events.set(page.items);
      this.changes.set(changeset.files);
      this.qualityGates.set(gates.items);
      this.workspace.set(workspace);
      this.continuations.set(continuations.items);
      this.repair.set(repair);
      this.artifacts.set(artifacts.items);
      await this.loadReviews();
      this.connectEvents();
    } catch (error) {
      this.error.set(apiErrorMessage(error, '运行加载失败'));
    } finally {
      this.loading.set(false);
    }
  }

  private async loadReviews(): Promise<void> {
    const page = await this.api.listReviews(1, 50);
    this.reviews.set(page.items.filter((review) => review.runId === this.runId));
  }

  private connectEvents(): void {
    if (this.destroyed || typeof EventSource === 'undefined') return;
    if (this.reconnectTimer !== undefined) {
      window.clearTimeout(this.reconnectTimer);
      this.reconnectTimer = undefined;
    }
    this.eventSource?.close();
    this.eventSource = new EventSource(
      `${this.apiBaseUrl}/runs/${this.runId}/events/stream?after_cursor=${this.lastEventCursor}`,
      {
        withCredentials: true,
      },
    );
    this.eventSource.onmessage = (message) => this.appendEvent(message.data);
    this.eventSource.addEventListener('run_started', (event) =>
      this.appendEvent((event as MessageEvent).data),
    );
    this.eventSource.addEventListener('turn_complete', (event) =>
      this.appendEvent((event as MessageEvent).data),
    );
    for (const eventType of [
      'continuation.created',
      'continuation.claimed',
      'continuation.dispatched',
      'continuation.cancelled',
      'continuation.completed',
    ]) {
      this.eventSource.addEventListener(eventType, () => {
        if (typeof this.api.listContinuations === 'function') {
          void this.api
            .listContinuations(undefined, this.runId, 1, 50)
            .then((page) => this.continuations.set(page.items))
            .catch(() => undefined);
        }
        void this.api
          .getRun(this.runId)
          .then((run) => this.run.set(run))
          .catch(() => undefined);
      });
    }
    for (const eventType of [
      'devrail.repair.created',
      'devrail.repair.dispatched',
      'devrail.repair.completed',
      'devrail.repair.handoff',
    ]) {
      this.eventSource.addEventListener(eventType, () => {
        const repairRequestId = this.run()?.repairRequestId;
        const request = repairRequestId
          ? this.api.getRepair(repairRequestId)
          : this.api
              .listRepairs(undefined, this.runId, 1, 50)
              .then((page) => page.items[0] ?? null);
        void request.then((repair) => this.repair.set(repair)).catch(() => undefined);
        void this.api
          .getRun(this.runId)
          .then((run) => this.run.set(run))
          .catch(() => undefined);
      });
    }
    this.eventSource.onerror = () => {
      this.eventSource?.close();
      this.eventSource = undefined;
      if (
        !this.destroyed &&
        !['completed', 'failed', 'cancelled'].includes(this.run()?.status ?? '')
      ) {
        this.reconnectTimer = window.setTimeout(() => {
          this.reconnectTimer = undefined;
          this.connectEvents();
        }, 1500);
      }
    };
  }

  private appendEvent(data: string): void {
    try {
      const event = JSON.parse(data) as DevRailRunEventResponse;
      this.events.update((items) =>
        items.some((item) => item.cursor === event.cursor) ? items : [...items, event],
      );
      this.lastEventCursor = Math.max(this.lastEventCursor, event.cursor);
    } catch {
      // SSE payloads are server-generated JSON; malformed data is ignored safely.
    }
  }
}
