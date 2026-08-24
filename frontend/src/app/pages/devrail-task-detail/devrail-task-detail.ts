import { ChangeDetectionStrategy, Component, OnInit, inject, signal } from '@angular/core';
import { DatePipe } from '@angular/common';
import { ActivatedRoute, RouterLink } from '@angular/router';
import { MatIconModule } from '@angular/material/icon';
import { MatProgressSpinnerModule } from '@angular/material/progress-spinner';
import { MatSnackBar, MatSnackBarModule } from '@angular/material/snack-bar';
import { apiErrorMessage } from '../../core/api-error';
import { DevRailApiService } from '../../features/devrail/data-access/devrail-api.service';
import type { DevRailTask } from '../../features/devrail/models/devrail.model';
import type {
  DevRailEnvironmentResponse,
  DevRailRepositoryResponse,
} from '../../generated/api/models';
import type { DevRailTaskCommentResponse } from '../../generated/api/models';

@Component({
  selector: 'app-devrail-task-detail',
  imports: [DatePipe, MatIconModule, MatProgressSpinnerModule, MatSnackBarModule, RouterLink],
  templateUrl: './devrail-task-detail.html',
  styleUrl: './devrail-task-detail.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class DevRailTaskDetailPage implements OnInit {
  readonly task = signal<DevRailTask | null>(null);
  readonly loading = signal(true);
  readonly busy = signal(false);
  readonly repositories = signal<DevRailRepositoryResponse[]>([]);
  readonly environments = signal<DevRailEnvironmentResponse[]>([]);
  readonly comments = signal<DevRailTaskCommentResponse[]>([]);
  private readonly route = inject(ActivatedRoute);
  private readonly api = inject(DevRailApiService);
  private readonly snack = inject(MatSnackBar);
  projectId = 0;
  private taskId = 0;

  ngOnInit(): void {
    this.projectId = Number(this.route.snapshot.paramMap.get('projectId'));
    this.taskId = Number(this.route.snapshot.paramMap.get('taskId'));
    void this.load();
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

  private async load(): Promise<void> {
    try {
      const [task, repositories, environments, comments] = await Promise.all([
        this.api.getTask(this.projectId, this.taskId),
        this.api.listRepositories(this.projectId),
        this.api.listEnvironments(this.projectId),
        this.api.listTaskComments(this.taskId),
      ]);
      this.task.set(task);
      this.repositories.set(repositories.items);
      this.environments.set(environments.items);
      this.comments.set(comments.items);
    } catch (error) {
      this.snack.open(apiErrorMessage(error, '任务加载失败'), '关闭', { duration: 5000 });
    } finally {
      this.loading.set(false);
    }
  }
}
