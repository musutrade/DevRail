export const DEVRAIL_PERMISSIONS = {
  projectRead: 'devrail:project:read',
  projectWrite: 'devrail:project:write',
  repositoryRead: 'devrail:repository:read',
  repositoryWrite: 'devrail:repository:write',
  environmentRead: 'devrail:environment:read',
  environmentWrite: 'devrail:environment:write',
  taskRead: 'devrail:task:read',
  taskWrite: 'devrail:task:write',
  taskDependencyRead: 'devrail:task_dependency:read',
  taskDependencyWrite: 'devrail:task_dependency:write',
  followupCreate: 'devrail:followup:create',
  runRead: 'devrail:run:read',
  runExecute: 'devrail:run:execute',
  runInterrupt: 'devrail:run:interrupt',
  runRetry: 'devrail:run:retry',
  workspaceRead: 'devrail:workspace:read',
  workspaceWrite: 'devrail:workspace:write',
} as const;

export const DEVRAIL_ROUTE_ACCESS = [DEVRAIL_PERMISSIONS.projectRead] as const;
