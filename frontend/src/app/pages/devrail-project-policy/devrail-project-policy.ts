import { ChangeDetectionStrategy, Component, OnInit, inject, signal } from '@angular/core';
import { ActivatedRoute, RouterLink } from '@angular/router';
import { MatIconModule } from '@angular/material/icon';
import { MatSnackBar, MatSnackBarModule } from '@angular/material/snack-bar';
import { apiErrorMessage } from '../../core/api-error';
import { DevRailApiService } from '../../features/devrail/data-access/devrail-api.service';

@Component({
  selector: 'app-devrail-project-policy',
  imports: [MatIconModule, MatSnackBarModule, RouterLink],
  templateUrl: './devrail-project-policy.html',
  styleUrl: './devrail-project-policy.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class DevRailProjectPolicyPage implements OnInit {
  readonly loading = signal(true);
  readonly busy = signal(false);
  readonly notificationPolicy = signal('{}');
  readonly qualityGateTemplate = signal('{}');
  private readonly route = inject(ActivatedRoute);
  private readonly api = inject(DevRailApiService);
  private readonly snack = inject(MatSnackBar);
  private projectId = 0;

  ngOnInit(): void {
    this.projectId = Number(this.route.snapshot.paramMap.get('id'));
    void this.load();
  }

  setNotificationPolicy(value: string): void {
    this.notificationPolicy.set(value);
  }

  setQualityGateTemplate(value: string): void {
    this.qualityGateTemplate.set(value);
  }

  async save(): Promise<void> {
    let notificationPolicy: unknown;
    let qualityGateTemplate: unknown;
    try {
      notificationPolicy = JSON.parse(this.notificationPolicy());
      qualityGateTemplate = JSON.parse(this.qualityGateTemplate());
    } catch {
      this.snack.open('策略内容必须是有效的 JSON', '关闭', { duration: 4000 });
      return;
    }
    this.busy.set(true);
    try {
      await this.api.updateProjectPolicy(this.projectId, {
        notificationPolicy,
        qualityGateTemplate,
      });
      this.snack.open('项目策略已保存', '关闭', { duration: 2500 });
    } catch (error) {
      this.snack.open(apiErrorMessage(error, '保存项目策略失败'), '关闭', { duration: 5000 });
    } finally {
      this.busy.set(false);
    }
  }

  private async load(): Promise<void> {
    this.loading.set(true);
    try {
      const policy = await this.api.getProjectPolicy(this.projectId);
      this.notificationPolicy.set(JSON.stringify(policy.notificationPolicy, null, 2));
      this.qualityGateTemplate.set(JSON.stringify(policy.qualityGateTemplate, null, 2));
    } catch (error) {
      this.snack.open(apiErrorMessage(error, '项目策略加载失败'), '关闭', { duration: 5000 });
    } finally {
      this.loading.set(false);
    }
  }
}
