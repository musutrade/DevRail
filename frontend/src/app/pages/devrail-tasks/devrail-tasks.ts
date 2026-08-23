import { DatePipe } from '@angular/common';
import { ChangeDetectionStrategy, Component, OnInit, inject, signal } from '@angular/core';
import { ActivatedRoute, RouterLink } from '@angular/router';
import { MatIconModule } from '@angular/material/icon';
import { MatProgressSpinnerModule } from '@angular/material/progress-spinner';
import { MatSnackBar, MatSnackBarModule } from '@angular/material/snack-bar';
import { apiErrorMessage } from '../../core/api-error';
import { DevRailApiService } from '../../features/devrail/data-access/devrail-api.service';
import type { DevRailTask } from '../../features/devrail/models/devrail.model';

@Component({
  selector: 'app-devrail-tasks',
  imports: [DatePipe, MatIconModule, MatProgressSpinnerModule, MatSnackBarModule, RouterLink],
  templateUrl: './devrail-tasks.html',
  styleUrl: './devrail-tasks.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class DevRailTasksPage implements OnInit {
  readonly tasks = signal<DevRailTask[]>([]);
  readonly loading = signal(true);
  readonly error = signal<string | null>(null);
  readonly keyword = signal('');
  readonly status = signal('');
  readonly page = signal(1);
  readonly pageSize = 20;
  readonly total = signal(0);
  private readonly route = inject(ActivatedRoute);
  private readonly api = inject(DevRailApiService);
  private readonly snack = inject(MatSnackBar);
  projectId = 0;

  ngOnInit(): void {
    this.projectId = Number(this.route.snapshot.paramMap.get('projectId'));
    void this.load();
  }

  setKeyword(value: string): void {
    this.keyword.set(value);
  }
  setStatus(value: string): void {
    this.status.set(value);
  }
  applyFilters(): void {
    this.page.set(1);
    void this.load();
  }
  previousPage(): void {
    if (this.page() > 1) {
      this.page.update((value) => value - 1);
      void this.load();
    }
  }
  nextPage(): void {
    if (this.page() * this.pageSize < this.total()) {
      this.page.update((value) => value + 1);
      void this.load();
    }
  }

  private async load(): Promise<void> {
    this.loading.set(true);
    this.error.set(null);
    try {
      const result = await this.api.listTasks(this.projectId, this.page(), this.pageSize, {
        keyword: this.keyword().trim(),
        status: this.status(),
      });
      this.tasks.set(result.items);
      this.total.set(result.total);
    } catch (error) {
      this.error.set(apiErrorMessage(error, '任务加载失败'));
      this.snack.open(this.error()!, '关闭', { duration: 5000 });
    } finally {
      this.loading.set(false);
    }
  }
}
