export const DEPARTMENT_PERMISSIONS = {
  read: 'organization:department:read',
  write: 'organization:department:write',
} as const;

export const DEPARTMENT_ROUTE_ACCESS = [DEPARTMENT_PERMISSIONS.read] as const;
