import { ChangeDetectionStrategy, Component, OnInit, inject, signal } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { MatIconModule } from '@angular/material/icon';
import { DevRailApiService } from '../../features/devrail/data-access/devrail-api.service';
import { apiErrorMessage } from '../../core/api-error';
import type { DevRailPushDeviceResponse } from '../../generated/api/models';

@Component({
  selector: 'app-devrail-notification-settings',
  imports: [FormsModule, MatIconModule],
  templateUrl: './devrail-notification-settings.html',
  styleUrl: './devrail-notification-settings.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class DevRailNotificationSettingsPage implements OnInit {
  readonly loading = signal(true);
  readonly saving = signal(false);
  readonly saved = signal(false);
  readonly error = signal<string | null>(null);
  readonly devices = signal<DevRailPushDeviceResponse[]>([]);
  readonly revoking = signal<number | null>(null);
  inAppEnabled = true;
  pushEnabled = false;
  eventTypes =
    'run.completed, run.failed, devrail.approval.requested, devrail.approval.approved, devrail.approval.rejected, devrail.approval.cancelled, devrail.approval.expired';
  private readonly api = inject(DevRailApiService);

  ngOnInit(): void {
    void this.load();
  }

  async save(): Promise<void> {
    this.saving.set(true);
    this.saved.set(false);
    this.error.set(null);
    try {
      await this.api.updateNotificationPreferences({
        inAppEnabled: this.inAppEnabled,
        pushEnabled: false,
        eventTypes: this.eventTypes
          .split(',')
          .map((value) => value.trim())
          .filter(Boolean),
      });
      this.pushEnabled = false;
      this.saved.set(true);
    } catch (error) {
      this.error.set(apiErrorMessage(error, '通知设置保存失败'));
    } finally {
      this.saving.set(false);
    }
  }

  async revokeDevice(id: number): Promise<void> {
    this.revoking.set(id);
    this.error.set(null);
    try {
      await this.api.revokePushDevice(id);
      this.devices.update((items) => items.filter((item) => item.id !== id));
    } catch (error) {
      this.error.set(apiErrorMessage(error, '推送设备撤销失败'));
    } finally {
      this.revoking.set(null);
    }
  }

  private async load(): Promise<void> {
    try {
      const preferences = await this.api.getNotificationPreferences();
      this.inAppEnabled = preferences.inAppEnabled;
      this.pushEnabled = preferences.pushEnabled;
      this.eventTypes = preferences.eventTypes.join(', ');
      this.devices.set(await this.api.listPushDevices());
    } catch (error) {
      this.error.set(apiErrorMessage(error, '通知设置加载失败'));
    } finally {
      this.loading.set(false);
    }
  }
}
