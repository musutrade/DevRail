import { Injectable, inject } from '@angular/core';
import { Api } from '../../generated/api/api';
import { assignUserRoles } from '../../generated/api/fn/users/assign-user-roles';
import { batchAssignUserRoles } from '../../generated/api/fn/users/batch-assign-user-roles';
import { batchDeleteUsers } from '../../generated/api/fn/users/batch-delete-users';
import { createUser } from '../../generated/api/fn/users/create-user';
import { deleteUser } from '../../generated/api/fn/users/delete-user';
import { listUsers, ListUsers$Params } from '../../generated/api/fn/users/list-users';
import { updateUser } from '../../generated/api/fn/users/update-user';
import { CreateUserRequest } from '../../generated/api/models/create-user-request';
import { UpdateUserRequest } from '../../generated/api/models/update-user-request';
import { UserResponse } from '../../generated/api/models/user-response';
import { User } from '../models';

const AVATAR_COLORS = ['#165dff', '#00b42a', '#ff7d00', '#f53f3f', '#722ed1'];

export type UserListQuery = ListUsers$Params &
  Required<Pick<ListUsers$Params, 'page' | 'pageSize' | 'sortBy' | 'sortDirection'>>;

export interface UserPage {
  items: User[];
  total: number;
  page: number;
  pageSize: number;
  roleOptions: string[];
}

@Injectable({
  providedIn: 'root',
})
export class UserApiService {
  private readonly api = inject(Api);

  async getUsers(query: UserListQuery): Promise<UserPage> {
    const result = await this.api.invoke(listUsers, query);
    return { ...result, items: result.items.map(mapUser) };
  }

  async createUser(request: CreateUserRequest, stepUpToken?: string): Promise<User> {
    const user = await this.api.invoke(createUser, {
      body: request,
      'X-Step-Up-Token': stepUpToken,
    });
    return mapUser(user);
  }

  async updateUser(id: string, request: UpdateUserRequest, stepUpToken?: string): Promise<User> {
    const user = await this.api.invoke(updateUser, {
      id: Number(id),
      body: request,
      'X-Step-Up-Token': stepUpToken,
    });
    return mapUser(user);
  }

  async deleteUser(id: string, stepUpToken: string): Promise<void> {
    await this.api.invoke(deleteUser, { id: Number(id), 'X-Step-Up-Token': stepUpToken });
  }

  async assignUserRoles(id: string, roleIds: string[], stepUpToken: string): Promise<void> {
    await this.api.invoke(assignUserRoles, {
      id: Number(id),
      'X-Step-Up-Token': stepUpToken,
      body: {
        roleIds: roleIds.map(Number),
      },
    });
  }

  async batchDeleteUsers(ids: readonly string[], stepUpToken: string): Promise<void> {
    await this.api.invoke(batchDeleteUsers, {
      'X-Step-Up-Token': stepUpToken,
      body: { userIds: ids.map(Number) },
    });
  }

  async batchAssignUserRoles(
    ids: readonly string[],
    roleIds: readonly string[],
    stepUpToken: string,
  ): Promise<void> {
    await this.api.invoke(batchAssignUserRoles, {
      'X-Step-Up-Token': stepUpToken,
      body: { userIds: ids.map(Number), roleIds: roleIds.map(Number) },
    });
  }
}

function mapUser(user: UserResponse): User {
  return {
    id: String(user.id),
    username: user.username,
    name: user.displayName,
    email: user.email ?? '',
    departmentId: user.departmentId,
    roles: user.roles,
    status: user.status,
    lastLogin: user.lastLoginAt,
    createdAt: user.createdAt,
    avatarColor: AVATAR_COLORS[user.id % AVATAR_COLORS.length],
  };
}
