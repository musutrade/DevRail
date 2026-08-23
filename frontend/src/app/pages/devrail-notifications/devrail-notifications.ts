import { DatePipe } from '@angular/common';
import { ChangeDetectionStrategy, Component, OnInit, inject, signal } from '@angular/core';
import { RouterLink } from '@angular/router';
import { MatIconModule } from '@angular/material/icon';
import { MatProgressSpinnerModule } from '@angular/material/progress-spinner';
import { DevRailApiService } from '../../features/devrail/data-access/devrail-api.service';
import { apiErrorMessage } from '../../core/api-error';
import type { DevRailNotificationResponse } from '../../generated/api/models';

@Component({
  selector: 'app-devrail-notifications',
  imports: [DatePipe, MatIconModule, MatProgressSpinnerModule, RouterLink],
  templateUrl: './devrail-notifications.html',
  styleUrl: './devrail-notifications.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class DevRailNotificationsPage implements OnInit {
  readonly notifications = signal<DevRailNotificationResponse[]>([]);
  readonly loading = signal(true);
  readonly error = signal<string | null>(null);
  readonly page = signal(1);
  readonly total = signal(0);
  readonly unread = signal(0);
  readonly pageSize = 20;
  private readonly api = inject(DevRailApiService);

  ngOnInit(): void {
    void this.load();
  }

  async markRead(notification: DevRailNotificationResponse): Promise<void> {
    if (notification.readAt) return;
    await this.api.markNotificationRead(notification.id);
    notification.readAt = new Date().toISOString();
    this.notifications.set([...this.notifications()]);
    this.unread.update((value) => Math.max(0, value - 1));
  }

  async markAllRead(): Promise<void> {
    if (!this.unread()) return;
    await this.api.markAllNotificationsRead();
    this.notifications.update((items) =>
      items.map((item) => ({ ...item, readAt: item.readAt ?? new Date().toISOString() })),
    );
    this.unread.set(0);
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
      const result = await this.api.listNotifications(this.page(), this.pageSize);
      this.notifications.set(result.items);
      this.total.set(result.total);
      this.unread.set(result.unread);
    } catch (error) {
      this.error.set(apiErrorMessage(error, '通知加载失败'));
    } finally {
      this.loading.set(false);
    }
  }
}
