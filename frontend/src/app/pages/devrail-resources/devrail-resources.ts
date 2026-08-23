import { ChangeDetectionStrategy, Component, OnInit, inject, signal } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { ActivatedRoute, RouterLink } from '@angular/router';
import { MatIconModule } from '@angular/material/icon';
import { MatProgressSpinnerModule } from '@angular/material/progress-spinner';
import { DevRailApiService } from '../../features/devrail/data-access/devrail-api.service';
import { apiErrorMessage } from '../../core/api-error';
import type {
  DevRailEnvironment,
  DevRailRepository,
} from '../../features/devrail/models/devrail.model';
import type {
  CreateDevRailEnvironmentRequest,
  CreateDevRailRepositoryRequest,
} from '../../generated/api/models';

@Component({
  selector: 'app-devrail-resources',
  imports: [FormsModule, MatIconModule, MatProgressSpinnerModule, RouterLink],
  templateUrl: './devrail-resources.html',
  styleUrl: './devrail-resources.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class DevRailResourcesPage implements OnInit {
  readonly loading = signal(true);
  readonly error = signal<string | null>(null);
  readonly repositories = signal<DevRailRepository[]>([]);
  readonly environments = signal<DevRailEnvironment[]>([]);
  readonly kind = signal<'repositories' | 'environments'>('repositories');
  readonly creating = signal(false);
  readonly createError = signal<string | null>(null);
  repositoryName = '';
  repositoryUrl = '';
  repositoryBranch = 'main';
  environmentName = '';
  workspaceRoot = '';
  networkMode = 'off';
  maxDurationSecs = 3600;
  projectId = 0;
  private readonly route = inject(ActivatedRoute);
  private readonly api = inject(DevRailApiService);
  ngOnInit(): void {
    this.projectId = Number(this.route.snapshot.paramMap.get('projectId'));
    this.kind.set(
      this.route.snapshot.url.some((s) => s.path === 'environments')
        ? 'environments'
        : 'repositories',
    );
    void this.load();
  }
  private async load(): Promise<void> {
    try {
      if (this.kind() === 'repositories')
        this.repositories.set((await this.api.listRepositories(this.projectId)).items);
      else this.environments.set((await this.api.listEnvironments(this.projectId)).items);
    } catch (e) {
      this.error.set(apiErrorMessage(e, '资源加载失败'));
    } finally {
      this.loading.set(false);
    }
  }

  async createRepository(): Promise<void> {
    const request: CreateDevRailRepositoryRequest = {
      name: this.repositoryName,
      remoteUrl: this.repositoryUrl,
      defaultBranch: this.repositoryBranch || 'main',
    };
    await this.create(async () => {
      await this.api.createRepository(this.projectId, request);
      this.repositoryName = '';
      this.repositoryUrl = '';
      await this.load();
    });
  }

  async createEnvironment(): Promise<void> {
    const request: CreateDevRailEnvironmentRequest = {
      name: this.environmentName,
      workspaceRoot: this.workspaceRoot,
      networkMode: this.networkMode,
      maxDurationSecs: this.maxDurationSecs,
      enabled: true,
    };
    await this.create(async () => {
      await this.api.createEnvironment(this.projectId, request);
      this.environmentName = '';
      this.workspaceRoot = '';
      await this.load();
    });
  }

  private async create(action: () => Promise<void>): Promise<void> {
    this.creating.set(true);
    this.createError.set(null);
    try {
      await action();
    } catch (error) {
      this.createError.set(apiErrorMessage(error, '创建资源失败'));
    } finally {
      this.creating.set(false);
    }
  }
}
