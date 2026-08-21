import { Injectable, inject } from '@angular/core';
import { Api } from '../../generated/api/api';
import { createRole } from '../../generated/api/fn/roles/create-role';
import { deleteRole } from '../../generated/api/fn/roles/delete-role';
import { getRolePermissions } from '../../generated/api/fn/roles/get-role-permissions';
import { listRoles } from '../../generated/api/fn/roles/list-roles';
import { updateRolePermissions } from '../../generated/api/fn/roles/update-role-permissions';
import { updateRole } from '../../generated/api/fn/roles/update-role';
import { CreateRoleRequest } from '../../generated/api/models/create-role-request';
import { RoleResponse } from '../../generated/api/models/role-response';
import { UpdateRoleRequest } from '../../generated/api/models/update-role-request';
import { Role, RolePermissionRow } from '../models';

@Injectable({
  providedIn: 'root',
})
export class RoleApiService {
  private readonly api = inject(Api);

  async getRoles(): Promise<Role[]> {
    const roles = await this.api.invoke(listRoles);
    return roles.map(mapRole);
  }

  async getRolePermissionRows(): Promise<RolePermissionRow[]> {
    const roles = await this.getRoles();
    return roles.map((role) => ({
      roleId: role.id,
      roleCode: role.code,
      roleName: role.name,
      usersAssigned: role.members,
      active: role.isActive,
      groupIds: role.permissionGroupIds,
    }));
  }

  async getAssignedPermissionIds(roleId: string): Promise<string[]> {
    const response = await this.api.invoke(getRolePermissions, { id: Number(roleId) });
    return response.permissionIds.map(String);
  }

  async createRole(request: CreateRoleRequest, stepUpToken?: string): Promise<Role> {
    const role = await this.api.invoke(createRole, {
      body: request,
      'X-Step-Up-Token': stepUpToken,
    });
    return mapRole(role);
  }

  async updateRole(id: string, request: UpdateRoleRequest, stepUpToken?: string): Promise<Role> {
    const role = await this.api.invoke(updateRole, {
      id: Number(id),
      body: request,
      'X-Step-Up-Token': stepUpToken,
    });
    return mapRole(role);
  }

  async deleteRole(id: string, stepUpToken: string): Promise<void> {
    await this.api.invoke(deleteRole, { id: Number(id), 'X-Step-Up-Token': stepUpToken });
  }

  async assignRolePermissions(
    roleId: string,
    permissionIds: string[],
    stepUpToken: string,
  ): Promise<void> {
    await this.api.invoke(updateRolePermissions, {
      id: Number(roleId),
      'X-Step-Up-Token': stepUpToken,
      body: {
        permissionIds: permissionIds.map(Number),
      },
    });
  }
}

function mapRole(role: RoleResponse): Role {
  return {
    id: String(role.id),
    code: role.code,
    name: role.name,
    category: role.category,
    icon: role.icon ?? 'badge',
    color: role.color,
    description: role.description ?? '',
    dataScope: role.dataScope,
    members: role.members,
    permissionGroupIds: role.permissionGroupIds.map(String),
    isActive: role.isActive,
  };
}
