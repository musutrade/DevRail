import { ChangeDetectionStrategy, Component, OnInit, inject, signal } from '@angular/core';
import { JsonPipe } from '@angular/common';
import { ActivatedRoute, RouterLink } from '@angular/router';
import { MatIconModule } from '@angular/material/icon';
import { MatSnackBar, MatSnackBarModule } from '@angular/material/snack-bar';
import { DevRailApiService } from '../../features/devrail/data-access/devrail-api.service';
import { apiErrorMessage } from '../../core/api-error';
import type { DevRailApprovalResponse } from '../../generated/api/models';

@Component({
  selector: 'app-devrail-approval-detail',
  imports: [JsonPipe, MatIconModule, MatSnackBarModule, RouterLink],
  templateUrl: './devrail-approval-detail.html',
  styleUrl: './devrail-approval-detail.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class DevRailApprovalDetailPage implements OnInit {
  readonly approval = signal<DevRailApprovalResponse | null>(null);
  readonly reason = signal('');
  readonly busy = signal(false);
  readonly loading = signal(true);
  approvalId = 0;
  private readonly route = inject(ActivatedRoute);
  private readonly api = inject(DevRailApiService);
  private readonly snack = inject(MatSnackBar);
  ngOnInit(): void {
    this.approvalId = Number(this.route.snapshot.paramMap.get('id'));
    void this.load();
  }
  setReason(value: string): void {
    this.reason.set(value);
  }
  async decide(decision: 'approve' | 'reject'): Promise<void> {
    if (this.busy() || this.approval()?.status !== 'pending') return;
    this.busy.set(true);
    try {
      const body = { reason: this.reason().trim() || null };
      const result =
        decision === 'approve'
          ? await this.api.approveApproval(this.approvalId, body)
          : await this.api.rejectApproval(this.approvalId, body);
      this.approval.set(result);
      this.snack.open(decision === 'approve' ? '审批已批准' : '审批已拒绝', '关闭', {
        duration: 2500,
      });
    } catch (e) {
      this.snack.open(apiErrorMessage(e, '审批处理失败'), '关闭', { duration: 5000 });
    } finally {
      this.busy.set(false);
    }
  }
  private async load(): Promise<void> {
    try {
      this.approval.set(await this.api.getApproval(this.approvalId));
    } catch (e) {
      this.snack.open(apiErrorMessage(e, '审批加载失败'), '关闭', { duration: 5000 });
    } finally {
      this.loading.set(false);
    }
  }
}
