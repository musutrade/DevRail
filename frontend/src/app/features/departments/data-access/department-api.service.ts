import { Injectable, inject } from '@angular/core';
import { Api } from '../../../generated/api/api';
import { createDepartment } from '../../../generated/api/fn/departments/create-department';
import { deleteDepartment } from '../../../generated/api/fn/departments/delete-department';
import { listDepartments } from '../../../generated/api/fn/departments/list-departments';
import { updateDepartment } from '../../../generated/api/fn/departments/update-department';
import { DepartmentResponse } from '../../../generated/api/models/department-response';
import {
  CreateDepartmentInput,
  Department,
  UpdateDepartmentInput,
} from '../models/department.model';

@Injectable({ providedIn: 'root' })
export class DepartmentApiService {
  private readonly api = inject(Api);

  async list(): Promise<Department[]> {
    return (await this.api.invoke(listDepartments)).map(mapDepartment);
  }

  async create(input: CreateDepartmentInput, stepUpToken: string): Promise<Department> {
    const department = await this.api.invoke(createDepartment, {
      body: input,
      'X-Step-Up-Token': stepUpToken,
    });
    return mapDepartment(department);
  }

  async update(id: number, input: UpdateDepartmentInput, stepUpToken: string): Promise<Department> {
    const department = await this.api.invoke(updateDepartment, {
      id,
      body: input,
      'X-Step-Up-Token': stepUpToken,
    });
    return mapDepartment(department);
  }

  async delete(id: number, stepUpToken: string): Promise<void> {
    await this.api.invoke(deleteDepartment, { id, 'X-Step-Up-Token': stepUpToken });
  }
}

function mapDepartment(department: DepartmentResponse): Department {
  return { ...department };
}
