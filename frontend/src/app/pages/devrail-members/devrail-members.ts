import { DatePipe } from '@angular/common';
import { ChangeDetectionStrategy, Component, OnInit, inject, signal } from '@angular/core';
import { ActivatedRoute, RouterLink } from '@angular/router';
import { MatIconModule } from '@angular/material/icon';
import { MatSnackBar, MatSnackBarModule } from '@angular/material/snack-bar';
import { DevRailApiService } from '../../features/devrail/data-access/devrail-api.service';
import { apiErrorMessage } from '../../core/api-error';
import type { DevRailProjectMemberResponse } from '../../generated/api/models';

@Component({
  selector: 'app-devrail-members',
  imports: [DatePipe, MatIconModule, MatSnackBarModule, RouterLink],
  templateUrl: './devrail-members.html',
  styleUrl: './devrail-members.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class DevRailMembersPage implements OnInit {
  readonly members = signal<DevRailProjectMemberResponse[]>([]);
  readonly loading = signal(true);
  readonly busy = signal(false);
  readonly username = signal('');
  readonly role = signal('developer');
  private readonly route = inject(ActivatedRoute);
  private readonly api = inject(DevRailApiService);
  private readonly snack = inject(MatSnackBar);
  private projectId = 0;
  ngOnInit(): void {
    this.projectId = Number(this.route.snapshot.paramMap.get('id'));
    void this.load();
  }
  setUsername(value: string): void {
    this.username.set(value);
  }
  setRole(value: string): void {
    this.role.set(value);
  }
  async add(): Promise<void> {
    const userId = Number(this.username());
    if (!Number.isInteger(userId) || userId < 1) {
      this.snack.open('请输入有效的用户 ID', '关闭', { duration: 3000 });
      return;
    }
    this.busy.set(true);
    try {
      await this.api.addMember(this.projectId, { userId, role: this.role() });
      this.username.set('');
      this.snack.open('成员已添加', '关闭', { duration: 2500 });
      await this.load();
    } catch (error) {
      this.snack.open(apiErrorMessage(error, '添加成员失败'), '关闭', { duration: 5000 });
    } finally {
      this.busy.set(false);
    }
  }
  async remove(member: DevRailProjectMemberResponse): Promise<void> {
    if (!confirm(`确定移除成员“${member.displayName}”吗？`)) return;
    this.busy.set(true);
    try {
      await this.api.removeMember(this.projectId, member.userId);
      this.snack.open('成员已移除', '关闭', { duration: 2500 });
      await this.load();
    } catch (error) {
      this.snack.open(apiErrorMessage(error, '移除成员失败'), '关闭', { duration: 5000 });
    } finally {
      this.busy.set(false);
    }
  }
  private async load(): Promise<void> {
    this.loading.set(true);
    try {
      this.members.set((await this.api.listMembers(this.projectId)).items);
    } catch (error) {
      this.snack.open(apiErrorMessage(error, '成员加载失败'), '关闭', { duration: 5000 });
    } finally {
      this.loading.set(false);
    }
  }
}
