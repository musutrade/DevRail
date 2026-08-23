import { ChangeDetectionStrategy, Component, OnInit, inject, signal } from '@angular/core';
import { ActivatedRoute, RouterLink } from '@angular/router';
import { MatIconModule } from '@angular/material/icon';
import { MatSnackBar, MatSnackBarModule } from '@angular/material/snack-bar';
import { DevRailApiService } from '../../features/devrail/data-access/devrail-api.service';
import { apiErrorMessage } from '../../core/api-error';
import type {
  DevRailEnvironment,
  DevRailRepository,
} from '../../features/devrail/models/devrail.model';

@Component({
  selector: 'app-devrail-resource-detail',
  imports: [MatIconModule, MatSnackBarModule, RouterLink],
  templateUrl: './devrail-resource-detail.html',
  styleUrl: './devrail-resource-detail.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class DevRailResourceDetailPage implements OnInit {
  readonly resource = signal<DevRailRepository | DevRailEnvironment | null>(null);
  readonly kind = signal<'repositories' | 'environments'>('repositories');
  readonly busy = signal(false);
  readonly health = signal<Awaited<ReturnType<DevRailApiService['healthCheckEnvironment']>> | null>(
    null,
  );
  projectId = 0;
  resourceId = 0;
  private readonly route = inject(ActivatedRoute);
  private readonly api = inject(DevRailApiService);
  private readonly snack = inject(MatSnackBar);
  ngOnInit(): void {
    this.projectId = Number(this.route.snapshot.paramMap.get('projectId'));
    this.resourceId = Number(this.route.snapshot.paramMap.get('resourceId'));
    this.kind.set(
      this.route.snapshot.url.some((s) => s.path === 'environments')
        ? 'environments'
        : 'repositories',
    );
    void this.load();
  }
  async save(form: HTMLFormElement): Promise<void> {
    const data = new FormData(form);
    this.busy.set(true);
    try {
      const body =
        this.kind() === 'repositories'
          ? {
              name: String(data.get('name') || ''),
              remoteUrl: String(data.get('remoteUrl') || ''),
              defaultBranch: String(data.get('defaultBranch') || 'main'),
            }
          : {
              name: String(data.get('name') || ''),
              workspaceRoot: String(data.get('workspaceRoot') || ''),
              networkMode: String(data.get('networkMode') || 'off'),
              maxDurationSecs: Number(data.get('maxDurationSecs') || 3600),
              enabled: data.get('enabled') === 'on',
            };
      const updated =
        this.kind() === 'repositories'
          ? await this.api.updateRepository(this.projectId, this.resourceId, body)
          : await this.api.updateEnvironment(this.projectId, this.resourceId, body);
      this.resource.set(updated);
      this.snack.open('资源已保存', '关闭', { duration: 2500 });
    } catch (e) {
      this.snack.open(apiErrorMessage(e, '资源保存失败'), '关闭', { duration: 5000 });
    } finally {
      this.busy.set(false);
    }
  }
  async syncRepository(): Promise<void> {
    this.busy.set(true);
    try {
      this.resource.set(await this.api.syncRepository(this.projectId, this.resourceId));
      this.snack.open('仓库同步检查已完成', '关闭', { duration: 2500 });
    } catch (e) {
      this.snack.open(apiErrorMessage(e, '仓库同步失败'), '关闭', { duration: 5000 });
    } finally {
      this.busy.set(false);
    }
  }
  async healthCheckEnvironment(): Promise<void> {
    this.busy.set(true);
    try {
      this.health.set(await this.api.healthCheckEnvironment(this.projectId, this.resourceId));
      this.snack.open('环境健康检查已完成', '关闭', { duration: 2500 });
    } catch (e) {
      this.snack.open(apiErrorMessage(e, '环境健康检查失败'), '关闭', { duration: 5000 });
    } finally {
      this.busy.set(false);
    }
  }
  private async load(): Promise<void> {
    try {
      this.resource.set(
        this.kind() === 'repositories'
          ? await this.api.getRepository(this.projectId, this.resourceId)
          : await this.api.getEnvironment(this.projectId, this.resourceId),
      );
    } catch (e) {
      this.snack.open(apiErrorMessage(e, '资源加载失败'), '关闭', { duration: 5000 });
    }
  }
}
