export type DepartmentStatus = 'active' | 'inactive';

export interface Department {
  readonly id: number;
  readonly organizationId: number;
  readonly parentId: number | null;
  readonly code: string;
  readonly name: string;
  readonly status: DepartmentStatus;
  readonly depth: number;
  readonly memberCount: number;
  readonly childCount: number;
  readonly createdAt: string;
  readonly updatedAt: string;
}

export interface CreateDepartmentInput {
  readonly parentId: number;
  readonly code: string;
  readonly name: string;
  readonly status: DepartmentStatus;
}

export interface UpdateDepartmentInput {
  readonly parentId?: number;
  readonly code: string;
  readonly name: string;
  readonly status: DepartmentStatus;
}
