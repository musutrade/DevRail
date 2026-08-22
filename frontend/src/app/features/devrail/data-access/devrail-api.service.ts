import { Injectable, inject } from '@angular/core';
import { Api } from '../../../generated/api/api';
import { archiveDevRailProject } from '../../../generated/api/fn/devrail/archive-dev-rail-project';
import { createDevRailEnvironment } from '../../../generated/api/fn/devrail/create-dev-rail-environment';
import { createDevRailProject } from '../../../generated/api/fn/devrail/create-dev-rail-project';
import { createDevRailRepository } from '../../../generated/api/fn/devrail/create-dev-rail-repository';
import { createDevRailTask } from '../../../generated/api/fn/devrail/create-dev-rail-task';
import { getDevRailEnvironment } from '../../../generated/api/fn/devrail/get-dev-rail-environment';
import { getDevRailProject } from '../../../generated/api/fn/devrail/get-dev-rail-project';
import { getDevRailRepository } from '../../../generated/api/fn/devrail/get-dev-rail-repository';
import { getDevRailTask } from '../../../generated/api/fn/devrail/get-dev-rail-task';
import { listDevRailEnvironments } from '../../../generated/api/fn/devrail/list-dev-rail-environments';
import { listDevRailProjects } from '../../../generated/api/fn/devrail/list-dev-rail-projects';
import { listDevRailRepositories } from '../../../generated/api/fn/devrail/list-dev-rail-repositories';
import { listDevRailTasks } from '../../../generated/api/fn/devrail/list-dev-rail-tasks';
import { updateDevRailEnvironment } from '../../../generated/api/fn/devrail/update-dev-rail-environment';
import { updateDevRailProject } from '../../../generated/api/fn/devrail/update-dev-rail-project';
import { updateDevRailRepository } from '../../../generated/api/fn/devrail/update-dev-rail-repository';
import { updateDevRailTask } from '../../../generated/api/fn/devrail/update-dev-rail-task';
import type {
  CreateDevRailEnvironmentRequest,
  CreateDevRailProjectRequest,
  CreateDevRailRepositoryRequest,
  CreateDevRailTaskRequest,
  DevRailEnvironmentPage,
  DevRailProjectPage,
  DevRailRepositoryPage,
  DevRailTaskPage,
  UpdateDevRailEnvironmentRequest,
  UpdateDevRailProjectRequest,
  UpdateDevRailRepositoryRequest,
  UpdateDevRailTaskRequest,
} from '../../../generated/api/models';

@Injectable({ providedIn: 'root' })
export class DevRailApiService {
  private readonly api = inject(Api);

  listProjects(page = 1, pageSize = 20): Promise<DevRailProjectPage> {
    return this.api.invoke(listDevRailProjects, { page, pageSize });
  }
  getProject(id: number) {
    return this.api.invoke(getDevRailProject, { id });
  }
  createProject(body: CreateDevRailProjectRequest) {
    return this.api.invoke(createDevRailProject, { body });
  }
  updateProject(id: number, body: UpdateDevRailProjectRequest) {
    return this.api.invoke(updateDevRailProject, { id, body });
  }
  archiveProject(id: number) {
    return this.api.invoke(archiveDevRailProject, { id });
  }

  listRepositories(projectId: number, page = 1, pageSize = 20): Promise<DevRailRepositoryPage> {
    return this.api.invoke(listDevRailRepositories, { project_id: projectId, page, pageSize });
  }
  getRepository(projectId: number, id: number) {
    return this.api.invoke(getDevRailRepository, { project_id: projectId, id });
  }
  createRepository(projectId: number, body: CreateDevRailRepositoryRequest) {
    return this.api.invoke(createDevRailRepository, { project_id: projectId, body });
  }
  updateRepository(projectId: number, id: number, body: UpdateDevRailRepositoryRequest) {
    return this.api.invoke(updateDevRailRepository, { project_id: projectId, id, body });
  }

  listEnvironments(projectId: number, page = 1, pageSize = 20): Promise<DevRailEnvironmentPage> {
    return this.api.invoke(listDevRailEnvironments, { project_id: projectId, page, pageSize });
  }
  getEnvironment(projectId: number, id: number) {
    return this.api.invoke(getDevRailEnvironment, { project_id: projectId, id });
  }
  createEnvironment(projectId: number, body: CreateDevRailEnvironmentRequest) {
    return this.api.invoke(createDevRailEnvironment, { project_id: projectId, body });
  }
  updateEnvironment(projectId: number, id: number, body: UpdateDevRailEnvironmentRequest) {
    return this.api.invoke(updateDevRailEnvironment, { project_id: projectId, id, body });
  }

  listTasks(projectId: number, page = 1, pageSize = 20): Promise<DevRailTaskPage> {
    return this.api.invoke(listDevRailTasks, { project_id: projectId, page, pageSize });
  }
  getTask(projectId: number, id: number) {
    return this.api.invoke(getDevRailTask, { project_id: projectId, id });
  }
  createTask(projectId: number, body: CreateDevRailTaskRequest) {
    return this.api.invoke(createDevRailTask, { project_id: projectId, body });
  }
  updateTask(projectId: number, id: number, body: UpdateDevRailTaskRequest) {
    return this.api.invoke(updateDevRailTask, { project_id: projectId, id, body });
  }
}
