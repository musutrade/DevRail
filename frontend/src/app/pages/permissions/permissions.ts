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
import { PermissionApiService } from '../../core/api/permission-api.service';
import { PermissionGroup, PermissionType } from '../../core/models';

const TYPE_META: Record<PermissionType, { icon: string; cls: string; label: string }> = {
  menu: { icon: 'table_chart', cls: 'badge-menu', label: '菜单' },
  button: { icon: 'smart_button', cls: 'badge-button', label: '按钮' },
  api: { icon: 'api', cls: 'badge-api', label: 'API' },
};

interface FilteredGroup extends PermissionGroup {
  collapsed: boolean;
}

@Component({
  selector: 'app-permissions',
  imports: [MatIconModule, MatProgressSpinnerModule],
  templateUrl: './permissions.html',
  styleUrl: './permissions.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class PermissionsPage implements OnInit {
  readonly groups = signal<PermissionGroup[]>([]);
  readonly loading = signal(true);
  readonly error = signal<string | null>(null);
  readonly search = signal('');
  readonly typeFilter = signal<'all' | PermissionType>('all');
  readonly collapsed = signal<Set<string>>(new Set());

  /** 类型元数据暴露给模板 */
  readonly typeMeta = TYPE_META;

  readonly typeOptions: ('all' | PermissionType)[] = ['all', 'menu', 'button', 'api'];
  private requestSequence = 0;

  private readonly permissionApi = inject(PermissionApiService);

  readonly filteredGroups = computed<FilteredGroup[]>(() => {
    const term = this.search().trim().toLowerCase();
    const type = this.typeFilter();
    const collapsedIds = this.collapsed();
    return this.groups()
      .map((g) => ({
        ...g,
        collapsed: collapsedIds.has(g.id),
        permissions: g.permissions.filter((p) => {
          const matchType = type === 'all' || p.type === type;
          const matchTerm =
            !term ||
            p.name.toLowerCase().includes(term) ||
            p.code.toLowerCase().includes(term) ||
            p.description.toLowerCase().includes(term);
          return matchType && matchTerm;
        }),
      }))
      .filter((g) => g.permissions.length > 0);
  });

  /** 当前筛选结果中的权限总数 */
  readonly totalCount = computed(() =>
    this.filteredGroups().reduce((acc, g) => acc + g.permissions.length, 0),
  );

  ngOnInit(): void {
    void this.loadGroups();
  }

  retry(): void {
    void this.loadGroups();
  }

  private async loadGroups(): Promise<void> {
    const requestSequence = ++this.requestSequence;
    this.loading.set(true);
    this.error.set(null);
    try {
      const groups = await this.permissionApi.getPermissionGroups();
      if (requestSequence !== this.requestSequence) {
        return;
      }
      this.groups.set(groups);
    } catch {
      if (requestSequence === this.requestSequence) {
        this.error.set('权限数据加载失败，请稍后重试');
      }
    } finally {
      if (requestSequence === this.requestSequence) {
        this.loading.set(false);
      }
    }
  }

  toggleGroup(id: string): void {
    this.collapsed.update((s) => {
      const next = new Set(s);
      if (next.has(id)) {
        next.delete(id);
      } else {
        next.add(id);
      }
      return next;
    });
  }

  searchPermissions(value: string): void {
    this.search.set(value);
  }

  applyTypeFilter(value: string): void {
    this.typeFilter.set(value as 'all' | PermissionType);
  }
}
