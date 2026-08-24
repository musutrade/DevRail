import { ChangeDetectionStrategy, Component, OnInit, inject, signal } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { MatIconModule } from '@angular/material/icon';
import { DevRailApiService } from '../../features/devrail/data-access/devrail-api.service';
import { apiErrorMessage } from '../../core/api-error';
import type {
  DevRailPushConfigResponse,
  DevRailPushDeviceResponse,
} from '../../generated/api/models';

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
  readonly pushConfig = signal<DevRailPushConfigResponse | null>(null);
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

  async enablePush(): Promise<void> {
    this.error.set(null);
    const config = this.pushConfig();
    if (!config?.enabled || !config.publicKey) {
      this.error.set('服务端尚未配置 Web Push');
      return;
    }
    if (
      !('serviceWorker' in navigator) ||
      !('PushManager' in window) ||
      !('Notification' in window)
    ) {
      this.error.set('当前浏览器不支持 Web Push');
      return;
    }
    try {
      if ((await Notification.requestPermission()) !== 'granted') {
        this.error.set('未获得浏览器通知权限');
        return;
      }
      const registration = await navigator.serviceWorker.register('/sw.js');
      const subscription = await registration.pushManager.subscribe({
        userVisibleOnly: true,
        applicationServerKey: this.decodeKey(config.publicKey),
      });
      const json = subscription.toJSON();
      const keys = json.keys ?? {};
      await this.api.registerPushDevice({
        deviceName: `${navigator.platform || '浏览器'} Web Push`,
        platform: /iPhone|iPad|iPod/i.test(navigator.userAgent) ? 'ios-pwa' : 'web-pwa',
        browser: navigator.userAgent.slice(0, 64),
        timezone: Intl.DateTimeFormat().resolvedOptions().timeZone,
        clientVersion: 'devrail-web-v1',
        endpoint: subscription.endpoint,
        p256dh: keys['p256dh'] ?? '',
        auth: keys['auth'] ?? '',
      });
      await this.api.updateNotificationPreferences({ pushEnabled: true });
      this.pushEnabled = true;
      this.devices.set(await this.api.listPushDevices());
    } catch (error) {
      this.error.set(apiErrorMessage(error, 'Web Push 初始化失败'));
    }
  }

  private async load(): Promise<void> {
    try {
      const preferences = await this.api.getNotificationPreferences();
      this.inAppEnabled = preferences.inAppEnabled;
      this.pushEnabled = preferences.pushEnabled;
      this.eventTypes = preferences.eventTypes.join(', ');
      this.pushConfig.set(await this.api.getPushConfig());
      this.devices.set(await this.api.listPushDevices());
    } catch (error) {
      this.error.set(apiErrorMessage(error, '通知设置加载失败'));
    } finally {
      this.loading.set(false);
    }
  }

  private decodeKey(value: string): ArrayBuffer {
    const normalized = (value + '='.repeat((4 - (value.length % 4)) % 4))
      .replace(/-/g, '+')
      .replace(/_/g, '/');
    const binary = window.atob(normalized);
    return Uint8Array.from(binary, (character) => character.charCodeAt(0)).buffer as ArrayBuffer;
  }
}
