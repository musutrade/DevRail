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
  readonly reviews = signal<DevRailReviewResponse[]>([]);
  readonly reviewerUserId = signal('');
  readonly reviewSummary = signal('');
  readonly reviewComments = signal<DevRailReviewCommentResponse[]>([]);
  readonly selectedReviewId = signal<number | null>(null);
  readonly externalReviewComments = signal<DevRailExternalReviewCommentResponse[]>([]);
  readonly workspace = signal<DevRailTaskWorkspaceResponse | null>(null);
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
  readonly canManageWorkspace = computed(() =>
    this.auth.hasPermission(DEVRAIL_PERMISSIONS.workspaceWrite),
  );
  private readonly route = inject(ActivatedRoute);
  private readonly auth = inject(AuthService);
  private readonly api = inject(DevRailApiService);
  private readonly snack = inject(MatSnackBar);
  private eventSource?: EventSource;
  private runId = 0;
  private lastEventCursor = 0;

  ngOnInit(): void {
    this.runId = Number(this.route.snapshot.paramMap.get('id'));
    void this.load();
  }

  ngOnDestroy(): void {
    this.eventSource?.close();
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
      link.download = patch.fileName;
      link.click();
      URL.revokeObjectURL(url);
      this.snack.open('补丁已下载', '关闭', { duration: 2500 });
    } catch (error) {
      this.snack.open(apiErrorMessage(error, '导出补丁失败'), '关闭', { duration: 5000 });
    } finally {
      this.busy.set(false);
    }
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

  private async load(): Promise<void> {
    this.loading.set(true);
    this.error.set(null);
    try {
      this.run.set(await this.api.getRun(this.runId));
      const workspacePromise =
        typeof this.api.getRunWorkspace === 'function'
          ? this.api.getRunWorkspace(this.runId).catch(() => null)
          : Promise.resolve(null);
      const [page, changeset, gates, workspace] = await Promise.all([
        this.api.listRunEvents(this.runId),
        this.api.getRunChangeset(this.runId),
        this.api.getRunQualityGates(this.runId),
        workspacePromise,
      ]);
      this.events.set(page.items);
      this.changes.set(changeset.files);
      this.qualityGates.set(gates.items);
      this.workspace.set(workspace);
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
    this.eventSource?.close();
    this.eventSource = new EventSource(
      `/api/v1/runs/${this.runId}/events/stream?after_cursor=${this.lastEventCursor}`,
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
    this.eventSource.onerror = () => {
      this.eventSource?.close();
      if (!['completed', 'failed', 'cancelled'].includes(this.run()?.status ?? '')) {
        window.setTimeout(() => this.connectEvents(), 1500);
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
