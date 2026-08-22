import { DatePipe } from '@angular/common';
import {
  ChangeDetectionStrategy,
  Component,
  OnInit,
  computed,
  inject,
  signal,
} from '@angular/core';
import { FormsModule } from '@angular/forms';
import { MatIconModule } from '@angular/material/icon';
import { MatProgressSpinnerModule } from '@angular/material/progress-spinner';
import { MatSnackBar, MatSnackBarModule } from '@angular/material/snack-bar';
import { AuthService } from '../../core/auth.service';
import { apiErrorMessage } from '../../core/api-error';
import { DevRailApiService } from '../../features/devrail/data-access/devrail-api.service';
import { DEVRAIL_PERMISSIONS } from '../../features/devrail/devrail.permissions';
import type { DevRailProject } from '../../features/devrail/models/devrail.model';

@Component({
  selector: 'app-devrail',
  imports: [DatePipe, FormsModule, MatIconModule, MatProgressSpinnerModule, MatSnackBarModule],
  templateUrl: './devrail.html',
  styleUrl: './devrail.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class DevRailPage implements OnInit {
  readonly projects = signal<DevRailProject[]>([]);
  readonly loading = signal(true);
  readonly busy = signal(false);
  readonly error = signal<string | null>(null);
  readonly canWrite = computed(() => this.auth.hasPermission(DEVRAIL_PERMISSIONS.projectWrite));
  readonly draft = signal({ slug: '', name: '', description: '' });
  private readonly auth = inject(AuthService);
  private readonly api = inject(DevRailApiService);
  private readonly snack = inject(MatSnackBar);

  ngOnInit(): void {
    void this.load();
  }
  setDraft(field: 'slug' | 'name' | 'description', value: string): void {
    this.draft.update((current) => ({ ...current, [field]: value }));
  }
  async create(): Promise<void> {
    const draft = this.draft();
    if (!draft.slug.trim() || !draft.name.trim()) {
      this.snack.open('请填写项目标识和名称', '关闭', { duration: 3000 });
      return;
    }
    this.busy.set(true);
    try {
      await this.api.createProject({
        slug: draft.slug.trim(),
        name: draft.name.trim(),
        description: draft.description.trim() || null,
      });
      this.draft.set({ slug: '', name: '', description: '' });
      this.snack.open('项目已创建', '关闭', { duration: 2500 });
      await this.load();
    } catch (error) {
      this.snack.open(apiErrorMessage(error, '项目创建失败'), '关闭', { duration: 5000 });
    } finally {
      this.busy.set(false);
    }
  }
  async archive(project: DevRailProject): Promise<void> {
    if (!confirm(`确定归档项目“${project.name}”吗？`)) return;
    this.busy.set(true);
    try {
      await this.api.archiveProject(project.id);
      this.snack.open('项目已归档', '关闭', { duration: 2500 });
      await this.load();
    } catch (error) {
      this.snack.open(apiErrorMessage(error, '项目归档失败'), '关闭', { duration: 5000 });
    } finally {
      this.busy.set(false);
    }
  }
  retry(): void {
    void this.load();
  }
  private async load(): Promise<void> {
    this.loading.set(true);
    this.error.set(null);
    try {
      this.projects.set((await this.api.listProjects()).items);
    } catch (error) {
      this.error.set(apiErrorMessage(error, '项目加载失败'));
    } finally {
      this.loading.set(false);
    }
  }
}
