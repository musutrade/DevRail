import { Injectable, inject } from '@angular/core';
import { Api } from '../../generated/api/api';
import { listPermissionGroups } from '../../generated/api/fn/permissions/list-permission-groups';
import { PermissionGroup } from '../models';

@Injectable({
  providedIn: 'root',
})
export class PermissionApiService {
  private readonly api = inject(Api);

  async getPermissionGroups(): Promise<PermissionGroup[]> {
    const groups = await this.api.invoke(listPermissionGroups);
    return groups.map((group) => ({
      id: String(group.id),
      code: group.code,
      name: group.name,
      icon: group.icon ?? 'folder',
      permissions: group.permissions.map((permission) => ({
        id: String(permission.id),
        code: permission.code,
        name: permission.name,
        type: permission.type,
        description: permission.description ?? '',
      })),
    }));
  }
}
