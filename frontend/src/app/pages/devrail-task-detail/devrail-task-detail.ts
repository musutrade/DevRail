import { ChangeDetectionStrategy, Component, OnInit, inject, signal } from '@angular/core';
import { ActivatedRoute, RouterLink } from '@angular/router';
import { MatIconModule } from '@angular/material/icon';
import { MatProgressSpinnerModule } from '@angular/material/progress-spinner';
import { MatSnackBar, MatSnackBarModule } from '@angular/material/snack-bar';
import { apiErrorMessage } from '../../core/api-error';
import { DevRailApiService } from '../../features/devrail/data-access/devrail-api.service';
import type { DevRailTask } from '../../features/devrail/models/devrail.model';

@Component({
  selector: 'app-devrail-task-detail',
  imports: [MatIconModule, MatProgressSpinnerModule, MatSnackBarModule, RouterLink],
  templateUrl: './devrail-task-detail.html',
  styleUrl: './devrail-task-detail.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class DevRailTaskDetailPage implements OnInit {
  readonly task = signal<DevRailTask | null>(null);
  readonly loading = signal(true);
  readonly busy = signal(false);
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
      });
      this.task.set(updated);
      this.snack.open('任务已保存', '关闭', { duration: 2500 });
    } catch (error) {
      this.snack.open(apiErrorMessage(error, '任务保存失败'), '关闭', { duration: 5000 });
    } finally {
      this.busy.set(false);
    }
  }

  private async load(): Promise<void> {
    try {
      this.task.set(await this.api.getTask(this.projectId, this.taskId));
    } catch (error) {
      this.snack.open(apiErrorMessage(error, '任务加载失败'), '关闭', { duration: 5000 });
    } finally {
      this.loading.set(false);
    }
  }
}
