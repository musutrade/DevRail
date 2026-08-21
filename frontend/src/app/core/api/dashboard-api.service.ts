import { Injectable, inject } from '@angular/core';
import { Api } from '../../generated/api/api';
import { getDashboardStats } from '../../generated/api/fn/dashboard/get-dashboard-stats';
import { StatCard } from '../models';

@Injectable({
  providedIn: 'root',
})
export class DashboardApiService {
  private readonly api = inject(Api);

  async getUserStats(): Promise<StatCard[]> {
    const stats = await this.api.invoke(getDashboardStats);
    return [
      { label: '用户总数', value: String(stats.totalUsers), icon: 'group' },
      { label: '启用用户', value: String(stats.activeUsers), icon: 'verified_user' },
      { label: '角色总数', value: String(stats.totalRoles), icon: 'badge' },
      { label: '已暂停用户', value: String(stats.suspendedUsers), icon: 'person_off' },
    ];
  }
}
