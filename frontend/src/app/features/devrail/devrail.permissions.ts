export const DEVRAIL_PERMISSIONS = {
  projectRead: 'devrail:project:read',
  projectWrite: 'devrail:project:write',
  repositoryRead: 'devrail:repository:read',
  repositoryWrite: 'devrail:repository:write',
  environmentRead: 'devrail:environment:read',
  environmentWrite: 'devrail:environment:write',
  taskRead: 'devrail:task:read',
  taskWrite: 'devrail:task:write',
} as const;

export const DEVRAIL_ROUTE_ACCESS = [DEVRAIL_PERMISSIONS.projectRead] as const;
