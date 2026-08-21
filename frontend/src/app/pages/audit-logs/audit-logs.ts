import { DatePipe } from '@angular/common';
import { Clipboard } from '@angular/cdk/clipboard';
import {
  ChangeDetectionStrategy,
  Component,
  OnInit,
  computed,
  inject,
  signal,
} from '@angular/core';
import { MatIconModule } from '@angular/material/icon';
import { MatProgressSpinnerModule } from '@angular/material/progress-spinner';
import { AuditLogApiService } from '../../core/api/audit-log-api.service';
import { apiErrorMessage } from '../../core/api-error';
import { AuditLogResponse } from '../../generated/api/models/audit-log-response';

const ACTION_LABELS: Record<string, string> = {
  'auth.login.success': '登录成功',
  'auth.login.failure': '登录失败',
  'auth.logout': '退出登录',
  'auth.session.revoked': '撤销认证会话',
  'auth.mfa.challenge.issued': '发起二次验证',
  'auth.mfa.enrollment.started': '开始注册认证因子',
  'auth.mfa.enrollment.completed': '完成注册认证因子',
  'auth.mfa.verify.success': '二次验证成功',
  'auth.mfa.verify.failure': '二次验证失败',
  'auth.mfa.factor.revoked': '撤销认证因子',
  'auth.mfa.recovery_code.used': '使用恢复码',
  'auth.mfa.recovery_codes.generated': '生成恢复码',
  'auth.mfa.policy.changed': '变更多因素认证策略',
  'user.create': '创建用户',
  'user.update': '更新用户',
  'user.delete': '删除用户',
  'user.roles.update': '变更用户角色',
  'user.password.change': '修改本人密码',
  'user.bootstrap_super_admin': '引导超级管理员',
  'role.create': '创建角色',
  'role.update': '更新角色',
  'role.delete': '删除角色',
  'role.permissions.update': '变更角色权限',
  'department.create': '创建部门',
  'department.update': '更新部门',
  'department.delete': '删除部门',
};

const ACTION_OPTIONS = Object.entries(ACTION_LABELS).map(([value, label]) => ({ value, label }));

@Component({
  selector: 'app-audit-logs',
  imports: [DatePipe, MatIconModule, MatProgressSpinnerModule],
  templateUrl: './audit-logs.html',
  styleUrl: './audit-logs.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class AuditLogs implements OnInit {
  private readonly auditLogApi = inject(AuditLogApiService);
  private readonly clipboard = inject(Clipboard);
  private requestSequence = 0;

  readonly logs = signal<AuditLogResponse[]>([]);
  readonly loading = signal(true);
  readonly error = signal<string | null>(null);
  readonly keyword = signal('');
  readonly action = signal('');
  readonly page = signal(1);
  readonly pageSize = 20;
  readonly total = signal(0);
  readonly nextCursor = signal<string | null>(null);
  readonly copiedTraceId = signal<string | null>(null);
  readonly totalPages = computed(() => Math.max(1, Math.ceil(this.total() / this.pageSize)));
  readonly actionOptions = ACTION_OPTIONS;
  private readonly pageCursors = new Map<number, string | undefined>([[1, undefined]]);

  ngOnInit(): void {
    void this.load();
  }

  search(value: string): void {
    this.keyword.set(value);
    this.resetPagination();
    void this.load();
  }

  filterAction(value: string): void {
    this.action.set(value);
    this.resetPagination();
    void this.load();
  }

  retry(): void {
    void this.load();
  }

  goToPage(page: number): void {
    const next = Math.min(Math.max(1, page), this.totalPages());
    if (next === this.page()) {
      return;
    }
    if (next > this.page() && !this.nextCursor()) {
      return;
    }
    if (next > this.page()) {
      this.pageCursors.set(next, this.nextCursor() ?? undefined);
    }
    this.page.set(next);
    void this.load();
  }

  actionLabel(action: string): string {
    return ACTION_LABELS[action] ?? action;
  }

  targetLabel(log: AuditLogResponse): string {
    const type =
      log.targetType === 'user'
        ? '用户'
        : log.targetType === 'role'
          ? '角色'
          : log.targetType === 'department'
            ? '部门'
            : log.targetType;
    return log.targetId === null ? type : `${type} #${log.targetId}`;
  }

  detailText(details: Record<string, unknown>): string {
    return Object.keys(details).length === 0 ? '无附加信息' : JSON.stringify(details);
  }

  copyTraceId(traceId: string): void {
    if (!this.clipboard.copy(traceId)) {
      return;
    }
    this.copiedTraceId.set(traceId);
    window.setTimeout(() => {
      if (this.copiedTraceId() === traceId) {
        this.copiedTraceId.set(null);
      }
    }, 2000);
  }

  private async load(): Promise<void> {
    const requestSequence = ++this.requestSequence;
    this.loading.set(true);
    this.error.set(null);
    try {
      const page = await this.auditLogApi.getAuditLogs(
        this.page(),
        this.pageSize,
        this.keyword(),
        this.action(),
        this.pageCursors.get(this.page()),
      );
      if (requestSequence !== this.requestSequence) {
        return;
      }
      this.logs.set(page.items);
      this.total.set(page.total);
      this.nextCursor.set(page.nextCursor ?? null);
    } catch (error) {
      if (requestSequence === this.requestSequence) {
        this.error.set(apiErrorMessage(error, '审计日志加载失败，请稍后重试'));
      }
    } finally {
      if (requestSequence === this.requestSequence) {
        this.loading.set(false);
      }
    }
  }

  private resetPagination(): void {
    this.page.set(1);
    this.pageCursors.clear();
    this.pageCursors.set(1, undefined);
    this.nextCursor.set(null);
  }
}
