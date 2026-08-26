import {
  ChangeDetectionStrategy,
  Component,
  OnDestroy,
  OnInit,
  computed,
  inject,
  signal,
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
  readonly dependencyDraft = signal<DevRailTaskDependencyInput[]>([]);
  readonly dependencyCandidates = signal<DevRailTaskResponse[]>([]);
  readonly dependencyCandidateId = signal<number | null>(null);
  readonly dependencyBusy = signal(false);
  readonly canEditDependencies = computed(
    () =>
      this.auth.hasPermission(DEVRAIL_PERMISSIONS.taskDependencyWrite) &&
      ['draft', 'queued'].includes(this.task()?.status ?? ''),
  );
  readonly editingCommentId = signal<number | null>(null);
  private readonly route = inject(ActivatedRoute);
  private readonly auth = inject(AuthService);
  private readonly api = inject(DevRailApiService);
  private readonly snack = inject(MatSnackBar);
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
      const [task, repositories, environments, comments, events, candidates] = await Promise.all([
        this.api.getTask(this.projectId, this.taskId),
        this.api.listRepositories(this.projectId),
        this.api.listEnvironments(this.projectId),
        this.api.listTaskComments(this.taskId),
        this.api.listTaskEvents(this.taskId),
        this.api.listTasks(this.projectId, 1, 100),
      ]);
      this.task.set(task);
      this.repositories.set(repositories.items);
      this.environments.set(environments.items);
      this.comments.set(comments.items);
      this.taskEvents.set(events.items);
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
    ]) {
      source.addEventListener(eventType, refresh);
    }
    source.onerror = () => {
      source.close();
    };
    this.eventSource = source;
  }
}
