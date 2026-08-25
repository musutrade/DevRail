import { Injectable, inject } from '@angular/core';
import { Api } from '../../../generated/api/api';
import { archiveDevRailProject } from '../../../generated/api/fn/devrail/archive-dev-rail-project';
import { createDevRailEnvironment } from '../../../generated/api/fn/devrail/create-dev-rail-environment';
import { createDevRailProject } from '../../../generated/api/fn/devrail/create-dev-rail-project';
import { createDevRailRepository } from '../../../generated/api/fn/devrail/create-dev-rail-repository';
import { createDevRailTask } from '../../../generated/api/fn/devrail/create-dev-rail-task';
import { getDevRailEnvironment } from '../../../generated/api/fn/devrail/get-dev-rail-environment';
import { getDevRailProject } from '../../../generated/api/fn/devrail/get-dev-rail-project';
import { getDevRailProjectPolicy } from '../../../generated/api/fn/devrail/get-dev-rail-project-policy';
import { getDevRailRepository } from '../../../generated/api/fn/devrail/get-dev-rail-repository';
import { getDevRailTask } from '../../../generated/api/fn/devrail/get-dev-rail-task';
import { listDevRailEnvironments } from '../../../generated/api/fn/devrail/list-dev-rail-environments';
import { listDevRailProjects } from '../../../generated/api/fn/devrail/list-dev-rail-projects';
import { listDevRailRepositories } from '../../../generated/api/fn/devrail/list-dev-rail-repositories';
import { listDevRailTasks } from '../../../generated/api/fn/devrail/list-dev-rail-tasks';
import { updateDevRailEnvironment } from '../../../generated/api/fn/devrail/update-dev-rail-environment';
import { updateDevRailProject } from '../../../generated/api/fn/devrail/update-dev-rail-project';
import { updateDevRailProjectPolicy } from '../../../generated/api/fn/devrail/update-dev-rail-project-policy';
import { updateDevRailRepository } from '../../../generated/api/fn/devrail/update-dev-rail-repository';
import { updateDevRailTask } from '../../../generated/api/fn/devrail/update-dev-rail-task';
import { getDevRailRun } from '../../../generated/api/fn/devrail/get-dev-rail-run';
import { listDevRailRunEvents } from '../../../generated/api/fn/devrail/list-dev-rail-run-events';
import { interruptDevRailRun } from '../../../generated/api/fn/devrail/interrupt-dev-rail-run';
import { retryDevRailRun } from '../../../generated/api/fn/devrail/retry-dev-rail-run';
import { getDevRailRunChangeset } from '../../../generated/api/fn/devrail/get-dev-rail-run-changeset';
import { getDevRailRunQualityGates } from '../../../generated/api/fn/devrail/get-dev-rail-run-quality-gates';
import { executeDevRailRunQualityGates } from '../../../generated/api/fn/devrail/execute-dev-rail-run-quality-gates';
import { getDevRailRunQualityGateLog } from '../../../generated/api/fn/devrail/get-dev-rail-run-quality-gate-log';
import { listDevRailProjectMembers } from '../../../generated/api/fn/devrail/list-dev-rail-project-members';
import { addDevRailProjectMember } from '../../../generated/api/fn/devrail/add-dev-rail-project-member';
import { removeDevRailProjectMember } from '../../../generated/api/fn/devrail/remove-dev-rail-project-member';
import { listDevRailApprovals } from '../../../generated/api/fn/devrail/list-dev-rail-approvals';
import { getDevRailApproval } from '../../../generated/api/fn/devrail/get-dev-rail-approval';
import { approveDevRailApproval } from '../../../generated/api/fn/devrail/approve-dev-rail-approval';
import { recoverDevRailApproval } from '../../../generated/api/fn/devrail/recover-dev-rail-approval';
import { rejectDevRailApproval } from '../../../generated/api/fn/devrail/reject-dev-rail-approval';
import { withdrawDevRailApproval } from '../../../generated/api/fn/devrail/withdraw-dev-rail-approval';
import { listDevRailNotifications } from '../../../generated/api/fn/devrail/list-dev-rail-notifications';
import { markDevRailNotificationRead } from '../../../generated/api/fn/devrail/mark-dev-rail-notification-read';
import { markAllDevRailNotificationsRead } from '../../../generated/api/fn/devrail/mark-all-dev-rail-notifications-read';
import { getDevRailNotificationPreferences } from '../../../generated/api/fn/devrail/get-dev-rail-notification-preferences';
import { updateDevRailNotificationPreferences } from '../../../generated/api/fn/devrail/update-dev-rail-notification-preferences';
import { listDevRailPushDevices } from '../../../generated/api/fn/devrail/list-dev-rail-push-devices';
import { registerDevRailPushDevice } from '../../../generated/api/fn/devrail/register-dev-rail-push-device';
import { revokeDevRailPushDevice } from '../../../generated/api/fn/devrail/revoke-dev-rail-push-device';
import { getDevRailPushConfig } from '../../../generated/api/fn/devrail/get-dev-rail-push-config';
import { syncDevRailRepository } from '../../../generated/api/fn/devrail/sync-dev-rail-repository';
import { healthCheckDevRailEnvironment } from '../../../generated/api/fn/devrail/health-check-dev-rail-environment';
import { inspectDevRailRepositoryWorktree } from '../../../generated/api/fn/devrail/inspect-dev-rail-repository-worktree';
import { listDevRailTaskComments } from '../../../generated/api/fn/devrail/list-dev-rail-task-comments';
import { createDevRailTaskComment } from '../../../generated/api/fn/devrail/create-dev-rail-task-comment';
import { updateDevRailTaskComment } from '../../../generated/api/fn/devrail/update-dev-rail-task-comment';
import { deleteDevRailTaskComment } from '../../../generated/api/fn/devrail/delete-dev-rail-task-comment';
import { getDevRailRepositorySync } from '../../../generated/api/fn/devrail/get-dev-rail-repository-sync';
import { listDevRailReviews } from '../../../generated/api/fn/devrail/list-dev-rail-reviews';
import { createDevRailReview } from '../../../generated/api/fn/devrail/create-dev-rail-review';
import { decideDevRailReview } from '../../../generated/api/fn/devrail/decide-dev-rail-review';
import { listDevRailReviewComments } from '../../../generated/api/fn/devrail/list-dev-rail-review-comments';
import { createDevRailReviewComment } from '../../../generated/api/fn/devrail/create-dev-rail-review-comment';
import { updateDevRailReviewComment } from '../../../generated/api/fn/devrail/update-dev-rail-review-comment';
import { exportDevRailRunPatch } from '../../../generated/api/fn/devrail/export-dev-rail-run-patch';
import { getDevRailGitProvider } from '../../../generated/api/fn/devrail/get-dev-rail-git-provider';
import { createDevRailPullRequest } from '../../../generated/api/fn/devrail/create-dev-rail-pull-request';
import { syncDevRailPullRequest } from '../../../generated/api/fn/devrail/sync-dev-rail-pull-request';
import { listDevRailExternalReviewComments } from '../../../generated/api/fn/devrail/list-dev-rail-external-review-comments';
import { syncDevRailExternalReviewComments } from '../../../generated/api/fn/devrail/sync-dev-rail-external-review-comments';
import type {
  CreateDevRailEnvironmentRequest,
  CreateDevRailProjectRequest,
  CreateDevRailRepositoryRequest,
  CreateDevRailTaskRequest,
  DevRailEnvironmentPage,
  DevRailProjectPage,
  DevRailProjectPolicyResponse,
  DevRailRepositoryPage,
  DevRailTaskPage,
  UpdateDevRailEnvironmentRequest,
  UpdateDevRailProjectRequest,
  UpdateDevRailProjectPolicyRequest,
  UpdateDevRailRepositoryRequest,
  UpdateDevRailTaskRequest,
  DevRailRunEventPage,
  DevRailRunResponse,
  DevRailChangesetResponse,
  DevRailQualityGatePage,
  DevRailQualityGateLogPage,
  RetryDevRailRunRequest,
  AddDevRailProjectMemberRequest,
  DevRailProjectMemberPage,
  DevRailProjectMemberResponse,
  DevRailApprovalPage,
  DevRailApprovalResponse,
  DevRailApprovalDecisionRequest,
  DevRailNotificationPage,
  DevRailNotificationPreferencesResponse,
  UpdateDevRailNotificationPreferencesRequest,
  DevRailWorktreeResponse,
  DevRailRepositorySyncResponse,
  DevRailPushDeviceResponse,
  RegisterDevRailPushDeviceRequest,
  DevRailPushConfigResponse,
  DevRailTaskCommentPage,
  DevRailTaskCommentResponse,
  CreateDevRailTaskCommentRequest,
  UpdateDevRailTaskCommentRequest,
  DevRailReviewPage,
  DevRailReviewResponse,
  CreateDevRailReviewRequest,
  DecideDevRailReviewRequest,
  DevRailReviewCommentResponse,
  CreateDevRailReviewCommentRequest,
  UpdateDevRailReviewCommentRequest,
  DevRailPatchExportResponse,
  DevRailGitProviderResponse,
  CreateDevRailPullRequestRequest,
  DevRailPullRequestResponse,
  SyncDevRailPullRequestRequest,
  SyncDevRailExternalReviewRequest,
  DevRailExternalReviewCommentResponse,
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
  getProjectPolicy(id: number): Promise<DevRailProjectPolicyResponse> {
    return this.api.invoke(getDevRailProjectPolicy, { id });
  }
  updateProjectPolicy(id: number, body: UpdateDevRailProjectPolicyRequest) {
    return this.api.invoke(updateDevRailProjectPolicy, { id, body });
  }

  listMembers(projectId: number): Promise<DevRailProjectMemberPage> {
    return this.api.invoke(listDevRailProjectMembers, { project_id: projectId });
  }
  addMember(
    projectId: number,
    body: AddDevRailProjectMemberRequest,
  ): Promise<DevRailProjectMemberResponse> {
    return this.api.invoke(addDevRailProjectMember, { project_id: projectId, body });
  }
  removeMember(projectId: number, userId: number): Promise<void> {
    return this.api.invoke(removeDevRailProjectMember, { project_id: projectId, user_id: userId });
  }
  listApprovals(page = 1, pageSize = 20): Promise<DevRailApprovalPage> {
    return this.api.invoke(listDevRailApprovals, { page, pageSize });
  }
  listReviews(page = 1, pageSize = 20): Promise<DevRailReviewPage> {
    return this.api.invoke(listDevRailReviews, { page, pageSize });
  }
  createReview(body: CreateDevRailReviewRequest): Promise<DevRailReviewResponse> {
    return this.api.invoke(createDevRailReview, { body });
  }
  decideReview(id: number, body: DecideDevRailReviewRequest): Promise<DevRailReviewResponse> {
    return this.api.invoke(decideDevRailReview, { id, body });
  }
  listReviewComments(reviewId: number): Promise<DevRailReviewCommentResponse[]> {
    return this.api.invoke(listDevRailReviewComments, { id: reviewId });
  }
  createReviewComment(
    reviewId: number,
    body: CreateDevRailReviewCommentRequest,
  ): Promise<DevRailReviewCommentResponse> {
    return this.api.invoke(createDevRailReviewComment, { id: reviewId, body });
  }
  updateReviewComment(
    id: number,
    body: UpdateDevRailReviewCommentRequest,
  ): Promise<DevRailReviewCommentResponse> {
    return this.api.invoke(updateDevRailReviewComment, { id, body });
  }
  exportRunPatch(id: number): Promise<DevRailPatchExportResponse> {
    return this.api.invoke(exportDevRailRunPatch, { id });
  }
  getGitProvider(projectId: number, id: number): Promise<DevRailGitProviderResponse> {
    return this.api.invoke(getDevRailGitProvider, { project_id: projectId, id });
  }
  createPullRequest(
    projectId: number,
    id: number,
    body: CreateDevRailPullRequestRequest,
  ): Promise<DevRailPullRequestResponse> {
    return this.api.invoke(createDevRailPullRequest, { project_id: projectId, id, body });
  }
  syncPullRequest(
    projectId: number,
    id: number,
    body: SyncDevRailPullRequestRequest,
  ): Promise<DevRailPullRequestResponse> {
    return this.api.invoke(syncDevRailPullRequest, { project_id: projectId, id, body });
  }
  listExternalReviewComments(reviewId: number): Promise<DevRailExternalReviewCommentResponse[]> {
    return this.api.invoke(listDevRailExternalReviewComments, { id: reviewId });
  }
  syncExternalReviewComments(
    reviewId: number,
    body: SyncDevRailExternalReviewRequest,
  ): Promise<DevRailExternalReviewCommentResponse[]> {
    return this.api.invoke(syncDevRailExternalReviewComments, { id: reviewId, body });
  }
  getApproval(id: number): Promise<DevRailApprovalResponse> {
    return this.api.invoke(getDevRailApproval, { id });
  }
  approveApproval(
    id: number,
    body: DevRailApprovalDecisionRequest,
  ): Promise<DevRailApprovalResponse> {
    return this.api.invoke(approveDevRailApproval, { id, body });
  }
  recoverApproval(id: number): Promise<DevRailApprovalResponse> {
    return this.api.invoke(recoverDevRailApproval, { id });
  }
  rejectApproval(
    id: number,
    body: DevRailApprovalDecisionRequest,
  ): Promise<DevRailApprovalResponse> {
    return this.api.invoke(rejectDevRailApproval, { id, body });
  }
  withdrawApproval(
    id: number,
    body: DevRailApprovalDecisionRequest,
  ): Promise<DevRailApprovalResponse> {
    return this.api.invoke(withdrawDevRailApproval, { id, body });
  }

  listNotifications(page = 1, pageSize = 20): Promise<DevRailNotificationPage> {
    return this.api.invoke(listDevRailNotifications, { page, pageSize });
  }
  markNotificationRead(id: number): Promise<void> {
    return this.api.invoke(markDevRailNotificationRead, { id });
  }
  markAllNotificationsRead(): Promise<void> {
    return this.api.invoke(markAllDevRailNotificationsRead, {});
  }
  getNotificationPreferences(): Promise<DevRailNotificationPreferencesResponse> {
    return this.api.invoke(getDevRailNotificationPreferences, {});
  }
  updateNotificationPreferences(
    body: UpdateDevRailNotificationPreferencesRequest,
  ): Promise<DevRailNotificationPreferencesResponse> {
    return this.api.invoke(updateDevRailNotificationPreferences, { body });
  }
  listPushDevices(): Promise<DevRailPushDeviceResponse[]> {
    return this.api.invoke(listDevRailPushDevices, {});
  }
  getPushConfig(): Promise<DevRailPushConfigResponse> {
    return this.api.invoke(getDevRailPushConfig, {});
  }
  registerPushDevice(body: RegisterDevRailPushDeviceRequest): Promise<DevRailPushDeviceResponse> {
    return this.api.invoke(registerDevRailPushDevice, { body });
  }
  revokePushDevice(id: number): Promise<void> {
    return this.api.invoke(revokeDevRailPushDevice, { id });
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
  syncRepository(projectId: number, id: number) {
    return this.api.invoke(syncDevRailRepository, { project_id: projectId, id });
  }
  getRepositorySync(
    projectId: number,
    repositoryId: number,
    environmentId?: number,
  ): Promise<DevRailRepositorySyncResponse> {
    return this.api.invoke(getDevRailRepositorySync, {
      project_id: projectId,
      id: repositoryId,
      environmentId,
    });
  }
  inspectRepositoryWorktree(
    projectId: number,
    repositoryId: number,
    environmentId: number,
  ): Promise<DevRailWorktreeResponse> {
    return this.api.invoke(inspectDevRailRepositoryWorktree, {
      project_id: projectId,
      id: repositoryId,
      environmentId,
    });
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
  healthCheckEnvironment(projectId: number, id: number) {
    return this.api.invoke(healthCheckDevRailEnvironment, { project_id: projectId, id });
  }

  listTasks(
    projectId: number,
    page = 1,
    pageSize = 20,
    filters: { keyword?: string; status?: string; assigneeUserId?: number; label?: string } = {},
  ): Promise<DevRailTaskPage> {
    return this.api.invoke(listDevRailTasks, {
      project_id: projectId,
      page,
      pageSize,
      keyword: filters.keyword || undefined,
      status: filters.status || undefined,
      assigneeUserId: filters.assigneeUserId,
      label: filters.label || undefined,
    });
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
  listTaskComments(taskId: number, page = 1, pageSize = 50): Promise<DevRailTaskCommentPage> {
    return this.api.invoke(listDevRailTaskComments, { task_id: taskId, page, pageSize });
  }
  createTaskComment(
    taskId: number,
    body: CreateDevRailTaskCommentRequest,
  ): Promise<DevRailTaskCommentResponse> {
    return this.api.invoke(createDevRailTaskComment, { task_id: taskId, body });
  }
  updateTaskComment(
    id: number,
    body: UpdateDevRailTaskCommentRequest,
  ): Promise<DevRailTaskCommentResponse> {
    return this.api.invoke(updateDevRailTaskComment, { id, body });
  }
  deleteTaskComment(id: number): Promise<void> {
    return this.api.invoke(deleteDevRailTaskComment, { id });
  }

  getRun(id: number): Promise<DevRailRunResponse> {
    return this.api.invoke(getDevRailRun, { id });
  }

  listRunEvents(id: number): Promise<DevRailRunEventPage> {
    return this.api.invoke(listDevRailRunEvents, { id });
  }
  getRunChangeset(id: number): Promise<DevRailChangesetResponse> {
    return this.api.invoke(getDevRailRunChangeset, { id });
  }
  getRunQualityGates(id: number): Promise<DevRailQualityGatePage> {
    return this.api.invoke(getDevRailRunQualityGates, { id });
  }
  getRunQualityGateLog(
    id: number,
    logRef: string,
    afterCursor = 0,
    limit = 100,
  ): Promise<DevRailQualityGateLogPage> {
    return this.api.invoke(getDevRailRunQualityGateLog, {
      id,
      log_ref: logRef,
      after_cursor: afterCursor,
      limit,
    });
  }
  executeRunQualityGates(id: number): Promise<DevRailQualityGatePage> {
    return this.api.invoke(executeDevRailRunQualityGates, { id });
  }

  interruptRun(id: number): Promise<DevRailRunResponse> {
    return this.api.invoke(interruptDevRailRun, { id });
  }

  retryRun(id: number, body: RetryDevRailRunRequest): Promise<DevRailRunResponse> {
    return this.api.invoke(retryDevRailRun, { id, body });
  }
}
