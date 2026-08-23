import { DatePipe } from '@angular/common';
import { ChangeDetectionStrategy, Component, OnInit, inject, signal } from '@angular/core';
import { RouterLink } from '@angular/router';
import { MatIconModule } from '@angular/material/icon';
import { MatProgressSpinnerModule } from '@angular/material/progress-spinner';
import { DevRailApiService } from '../../features/devrail/data-access/devrail-api.service';
import { apiErrorMessage } from '../../core/api-error';
import type { DevRailApprovalResponse } from '../../generated/api/models';

@Component({
  selector: 'app-devrail-approvals',
  imports: [DatePipe, MatIconModule, MatProgressSpinnerModule, RouterLink],
  templateUrl: './devrail-approvals.html',
  styleUrl: './devrail-approvals.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class DevRailApprovalsPage implements OnInit {
  readonly approvals = signal<DevRailApprovalResponse[]>([]);
  readonly loading = signal(true);
  readonly error = signal<string | null>(null);
  readonly page = signal(1);
  readonly total = signal(0);
  readonly pageSize = 20;
  private readonly api = inject(DevRailApiService);
  ngOnInit(): void {
    void this.load();
  }
  previousPage(): void {
    if (this.page() > 1) {
      this.page.update((v) => v - 1);
      void this.load();
    }
  }
  nextPage(): void {
    if (this.page() * this.pageSize < this.total()) {
      this.page.update((v) => v + 1);
      void this.load();
    }
  }
  private async load(): Promise<void> {
    this.loading.set(true);
    try {
      const result = await this.api.listApprovals(this.page(), this.pageSize);
      this.approvals.set(result.items);
      this.total.set(result.total);
    } catch (e) {
      this.error.set(apiErrorMessage(e, '审批加载失败'));
    } finally {
      this.loading.set(false);
    }
  }
}
