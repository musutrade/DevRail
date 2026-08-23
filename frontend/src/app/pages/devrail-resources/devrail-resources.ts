import { ChangeDetectionStrategy, Component, OnInit, inject, signal } from '@angular/core';
import { ActivatedRoute, RouterLink } from '@angular/router';
import { MatIconModule } from '@angular/material/icon';
import { MatProgressSpinnerModule } from '@angular/material/progress-spinner';
import { DevRailApiService } from '../../features/devrail/data-access/devrail-api.service';
import { apiErrorMessage } from '../../core/api-error';
import type {
  DevRailEnvironment,
  DevRailRepository,
} from '../../features/devrail/models/devrail.model';

@Component({
  selector: 'app-devrail-resources',
  imports: [MatIconModule, MatProgressSpinnerModule, RouterLink],
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
}
