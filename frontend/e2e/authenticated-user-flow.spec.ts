import { expect, test } from '@playwright/test';
import type {
  ChangePasswordRequest,
  CreateUserRequest,
  RoleResponse as ApiRole,
  StepUpRequest,
  UpdateRoleRequest,
  UpdateUserRequest,
  UserResponse as ApiUser,
} from '../src/app/generated/api/models';

const administrator: ApiUser = {
  id: 1,
  username: 'admin',
  displayName: '管理员',
  email: 'admin@example.test',
  departmentId: 1,
  status: 'active',
  roles: ['超级管理员'],
  lastLoginAt: '2026-08-01T00:00:00Z',
  createdAt: '2026-08-01T00:00:00Z',
};

const permissions = [
  'dashboard:analytics:read',
  'organization:department:read',
  'permission:directory:read',
  'role:directory:read',
  'role:permissions:write',
  'role:write',
  'user:admin:deactivate',
  'user:admin:reset_password',
  'user:directory:read',
  'user:roles:write',
  'user:super_admin:grant',
  'user:write',
  'audit:logs:read',
];

let roles: ApiRole[] = [
  {
    id: 1,
    code: 'super_admin',
    name: '超级管理员',
    category: '系统核心',
    icon: 'shield',
    color: 'primary',
    description: '拥有所有系统权限',
    dataScope: 'all',
    isActive: true,
    members: 1,
    permissionGroupIds: [1],
  },
  {
    id: 2,
    code: 'viewer',
    name: '查看者',
    category: '只读',
    icon: 'visibility',
    color: 'success',
    description: '仅可查看系统内容',
    dataScope: 'self',
    isActive: true,
    members: 0,
    permissionGroupIds: [1],
  },
];

test('logs in, uses permission-aware navigation, and creates a user', async ({
  page,
}, testInfo) => {
  test.setTimeout(60_000);
  let users: ApiUser[] = [administrator];
  let createdRequest: Record<string, unknown> | null = null;
  let passwordChangeRequest: ChangePasswordRequest | null = null;
  const stepUpRequests: StepUpRequest[] = [];
  let roleStatusUpdateRequest: UpdateRoleRequest | null = null;
  let statusUpdateRequest: UpdateUserRequest | null = null;

  await page.route('**/config.js', async (route) => {
    await route.fulfill({
      contentType: 'application/javascript',
      body: `window.__ARC_ADMIN_CONFIG__ = {
        appName: '股票分析系统',
        appShortName: '投研平台',
        appSlug: 'stock-analysis',
        apiBaseUrl: '/api/v1',
        themeStorageKey: 'stock-analysis-theme'
      };`,
    });
  });

  await page.route('**/api/v1/**', async (route) => {
    const request = route.request();
    const path = new URL(request.url()).pathname;
    if (path === '/api/v1/auth/login') {
      await route.fulfill({
        json: {
          status: 'authenticated',
          expiresAt: '2026-08-01T08:00:00Z',
          user: administrator,
          methods: [],
          recoveryCodes: [],
        },
      });
    } else if (path === '/api/v1/auth/me') {
      await route.fulfill({ json: administrator });
    } else if (path === '/api/v1/auth/me/permissions') {
      await route.fulfill({ json: { codes: permissions } });
    } else if (path === '/api/v1/auth/me/mfa') {
      await route.fulfill({
        json: {
          passkeys: [],
          recoveryCodesRemaining: 10,
          required: false,
          totpEnabled: true,
        },
      });
    } else if (path === '/api/v1/auth/me/module-unlocks' && request.method() === 'POST') {
      const payload = request.postDataJSON() as { module: string };
      await route.fulfill({
        json: {
          module: payload.module,
          unlocked: true,
          expiresAt: '2026-08-01T00:05:00Z',
        },
      });
    } else if (/^\/api\/v1\/auth\/me\/module-unlocks\/(users|roles)$/.test(path)) {
      const module = path.split('/').at(-1);
      await route.fulfill({
        json: {
          module,
          unlocked: request.method() === 'GET' ? false : true,
          expiresAt: request.method() === 'GET' ? null : '2026-08-01T00:05:00Z',
        },
      });
    } else if (path === '/api/v1/auth/logout') {
      await route.fulfill({ status: 204 });
    } else if (path === '/api/v1/auth/me/step-up' && request.method() === 'POST') {
      const payload = request.postDataJSON() as StepUpRequest;
      stepUpRequests.push(payload);
      if (payload.totpCode === '000000') {
        await route.fulfill({
          status: 422,
          json: {
            error: {
              code: 'VALIDATION_ERROR',
              message: '身份验证器验证码不正确',
              traceId: 'step-up-validation',
            },
          },
        });
      } else {
        await route.fulfill({
          json: { token: 'step-up-token', expiresAt: '2026-08-01T00:05:00Z' },
        });
      }
    } else if (path === '/api/v1/departments') {
      await route.fulfill({
        json: [
          {
            id: 1,
            organizationId: 1,
            parentId: null,
            code: 'headquarters',
            name: '总部',
            status: 'active',
            depth: 0,
            memberCount: 1,
            childCount: 0,
            createdAt: '2026-08-01T00:00:00Z',
            updatedAt: '2026-08-01T00:00:00Z',
          },
        ],
      });
    } else if (path === '/api/v1/auth/me/password' && request.method() === 'PUT') {
      passwordChangeRequest = request.postDataJSON() as ChangePasswordRequest;
      await route.fulfill({ status: 204 });
    } else if (path === '/api/v1/permissions/groups') {
      await route.fulfill({
        json: [
          {
            id: 1,
            code: 'user-management',
            name: '用户管理',
            icon: 'group',
            permissions: [
              {
                id: 1,
                code: 'user:directory:read',
                name: '查看用户目录',
                type: 'menu',
                description: '查看用户列表和基础资料',
              },
              {
                id: 2,
                code: 'user:write',
                name: '维护用户',
                type: 'button',
                description: '创建和编辑用户',
              },
              {
                id: 3,
                code: 'user:api:read',
                name: '读取用户接口',
                type: 'api',
                description: '调用用户查询接口',
              },
            ],
          },
        ],
      });
    } else if (path === '/api/v1/dashboard/stats') {
      await route.fulfill({
        json: {
          totalUsers: users.length,
          activeUsers: users.length,
          totalRoles: 2,
          totalPermissions: 9,
          suspendedUsers: 0,
        },
      });
    } else if (path === '/api/v1/audit-logs') {
      await route.fulfill({
        json: {
          items: [
            {
              id: 1,
              actorUserId: 1,
              actorUsername: 'admin',
              action: 'user.roles.update',
              targetType: 'user',
              targetId: 2,
              details: { roleIds: [2] },
              traceId: 'audit-trace-e2e',
              createdAt: '2026-08-08T00:00:00Z',
            },
          ],
          total: 1,
          page: 1,
          pageSize: 20,
        },
      });
    } else if (path === '/api/v1/roles') {
      await route.fulfill({ json: roles });
    } else if (/^\/api\/v1\/roles\/\d+$/.test(path) && request.method() === 'PUT') {
      const roleId = Number(path.split('/').at(-1));
      const payload = request.postDataJSON() as UpdateRoleRequest;
      roleStatusUpdateRequest = payload;
      roles = roles.map((role) =>
        role.id === roleId ? { ...role, isActive: payload.isActive ?? role.isActive } : role,
      );
      await route.fulfill({ json: roles.find((role) => role.id === roleId) });
    } else if (path === '/api/v1/users' && request.method() === 'POST') {
      const payload = request.postDataJSON() as CreateUserRequest;
      createdRequest = payload as unknown as Record<string, unknown>;
      const created: ApiUser = {
        id: 2,
        username: payload.username,
        displayName: payload.displayName,
        email: payload.email ?? null,
        departmentId: payload.departmentId ?? null,
        status: payload.status ?? 'active',
        roles: ['查看者'],
        lastLoginAt: null,
        createdAt: '2026-08-04T00:00:00Z',
      };
      users = [...users, created];
      roles = roles.map((role) => (role.id === 2 ? { ...role, members: role.members + 1 } : role));
      await route.fulfill({ status: 201, json: created });
    } else if (/^\/api\/v1\/users\/\d+$/.test(path) && request.method() === 'PUT') {
      const userId = Number(path.split('/').at(-1));
      const payload = request.postDataJSON() as UpdateUserRequest;
      statusUpdateRequest = payload;
      users = users.map((user) =>
        user.id === userId ? { ...user, status: payload.status ?? user.status } : user,
      );
      await route.fulfill({ json: users.find((user) => user.id === userId) });
    } else if (path === '/api/v1/users') {
      await route.fulfill({
        json: {
          items: users,
          total: users.length,
          page: 1,
          pageSize: 10,
          roleOptions: ['超级管理员', '查看者'],
        },
      });
    } else {
      await route.fulfill({
        status: 404,
        json: { error: { code: 'NOT_FOUND', message: path } },
      });
    }
  });

  const settlePageAnimation = async () => {
    const pageSurface = page.locator('.page').first();
    await pageSurface.evaluate(async (element) => {
      await Promise.allSettled(
        element.getAnimations({ subtree: true }).map((animation) => animation.finished),
      );
    });
  };

  await page.goto('/login');
  await expect(page).toHaveTitle('股票分析系统');
  await expect(page.locator('html')).toHaveAttribute('data-app-slug', 'stock-analysis');
  await expect(page.locator('.login-header p')).toContainText('股票分析系统');
  if (process.env['VISUAL_REVIEW']) {
    await page.locator('.login-card').evaluate(async (element) => {
      await Promise.allSettled(
        element.getAnimations({ subtree: true }).map((animation) => animation.finished),
      );
    });
    await page.screenshot({ path: testInfo.outputPath('login.png'), fullPage: true });
  }
  await page.getByLabel('用户名').fill('admin');
  await page.getByLabel('密码', { exact: true }).fill('safe-password');
  await page.getByRole('button', { name: '登录' }).click();
  await expect(page.getByRole('heading', { name: '权限目录' })).toBeVisible();
  await expect(page.getByRole('table', { name: '权限目录列表' })).toBeVisible();
  await expect(page.locator('.sidebar-logo h2')).toHaveText('股票分析系统');
  await expect(page.locator('.sidebar-logo p')).toHaveText('投研平台');
  await expect(page.locator('.topbar-title')).toHaveText('股票分析系统');
  if (process.env['VISUAL_REVIEW']) {
    await settlePageAnimation();
    await page.screenshot({ path: testInfo.outputPath('permissions.png'), fullPage: true });
  }
  const skipLink = page.getByRole('link', { name: '跳到主要内容' });
  await skipLink.focus();
  await expect(skipLink).toBeFocused();
  await expect(skipLink).toBeVisible();
  await skipLink.press('Enter');
  await expect(page.locator('#main-content')).toBeFocused();

  const sidebarToggle = page.getByRole('button', { name: '收起侧边栏' });
  const userNavigationGroup = page
    .locator('.sidebar')
    .getByRole('button', { name: '用户管理', exact: true });
  if (await sidebarToggle.isVisible()) {
    await sidebarToggle.click();
    await userNavigationGroup.click();
    await expect(userNavigationGroup).toHaveAttribute('aria-expanded', 'true');
    await expect(page.getByRole('link', { name: '用户列表' })).toBeVisible();
  }

  await page.getByRole('button', { name: '账户菜单' }).click();
  await expect(page.getByRole('menuitem', { name: '修改密码' })).toBeVisible();
  await expect(page.getByRole('menuitem', { name: '退出登录' })).toBeVisible();
  if (process.env['VISUAL_REVIEW']) {
    await page.screenshot({ path: testInfo.outputPath('account-menu.png') });
  }
  await page.getByRole('menuitem', { name: '修改密码' }).click();

  const completeStepUp = async (title: string) => {
    const stepUpDialog = page.getByRole('dialog');
    await expect(stepUpDialog.getByRole('heading', { name: title })).toBeVisible();
    await stepUpDialog.getByLabel('当前密码').fill('safe-password');
    await stepUpDialog.getByLabel('身份验证器验证码').fill('123456');
    await stepUpDialog.getByRole('button', { name: '继续' }).click();
    await expect(stepUpDialog).toBeHidden();
  };

  const changePasswordDialog = page.getByRole('dialog');
  await expect(changePasswordDialog.getByRole('heading', { name: '修改密码' })).toBeVisible();
  await changePasswordDialog.locator('.editor-dialog').evaluate(async (element) => {
    await Promise.allSettled(
      element.getAnimations({ subtree: true }).map((animation) => animation.finished),
    );
  });
  const currentPassword = changePasswordDialog.locator('#current-password');
  const newPassword = changePasswordDialog.locator('#new-password');
  const confirmPassword = changePasswordDialog.locator('#confirm-password');
  const totpCode = changePasswordDialog.locator('#totp-code');
  await currentPassword.fill('safe-password');
  await newPassword.fill('updated-safe-password');
  await confirmPassword.fill('different-password');
  await confirmPassword.blur();
  await expect(changePasswordDialog.getByText('两次输入的新密码不一致')).toBeVisible();
  await confirmPassword.fill('updated-safe-password');
  await confirmPassword.blur();
  await expect(changePasswordDialog.getByText('两次输入的新密码不一致')).toBeHidden();
  await totpCode.fill('000000');
  await totpCode.blur();
  const savePassword = changePasswordDialog.getByRole('button', { name: '保存修改' });
  await expect(savePassword).toBeEnabled({ timeout: 10_000 });

  if (process.env['VISUAL_REVIEW']) {
    await page.screenshot({ path: testInfo.outputPath('change-password-dialog.png') });
  }

  const rejectedStepUp = page.waitForResponse(
    (response) =>
      new URL(response.url()).pathname === '/api/v1/auth/me/step-up' && response.status() === 422,
  );
  await savePassword.click();
  await rejectedStepUp;
  await expect(changePasswordDialog.getByText('身份验证器验证码不正确')).toBeVisible();
  await expect(changePasswordDialog).toBeVisible();
  await expect(page).not.toHaveURL(/\/login$/);
  expect(passwordChangeRequest).toBeNull();
  await totpCode.fill('123456');
  await savePassword.click();
  await expect(changePasswordDialog).toBeHidden();
  expect(passwordChangeRequest).toEqual({
    currentPassword: 'safe-password',
    newPassword: 'updated-safe-password',
  });
  await expect(page.getByText('密码修改成功，请重新登录', { exact: true })).toBeVisible();
  await expect(page).toHaveURL(/\/login$/);
  await page.getByRole('button', { name: '关闭' }).click();
  await page.getByLabel('用户名').fill('admin');
  await page.getByLabel('密码', { exact: true }).fill('updated-safe-password');
  await page.getByRole('button', { name: '登录' }).click();
  await expect(page.getByRole('heading', { name: '权限目录' })).toBeVisible();

  const mobileMenu = page.getByRole('button', { name: '打开菜单' });
  if (await mobileMenu.isVisible()) {
    await mobileMenu.click();
    await expect(page.locator('.mobile-menu-btn')).toHaveAttribute('aria-expanded', 'true');
    await expect
      .poll(() => page.evaluate(() => document.activeElement?.closest('.sidebar') !== null))
      .toBe(true);
    await page.keyboard.press('Escape');
    await expect(page.locator('.mobile-menu-btn')).toHaveAttribute('aria-expanded', 'false');
    await expect(page.locator('.mobile-menu-btn')).toBeFocused();
    await mobileMenu.click();
  }
  await page.getByRole('link', { name: '审计日志' }).click();
  await expect(page.getByRole('heading', { name: '审计日志' })).toBeVisible();
  const auditList =
    page.viewportSize()!.width < 800
      ? page.locator('.audit-mobile-list')
      : page.getByRole('table', { name: '审计日志列表' });
  await expect(auditList.getByText('变更用户角色', { exact: true })).toBeVisible();
  await expect(auditList.getByText('用户 #2', { exact: true })).toBeVisible();
  await expect(auditList.getByText('audit-trace-e2e', { exact: true })).toBeVisible();
  const copyTraceId = auditList.getByRole('button', { name: '复制追踪号 audit-trace-e2e' });
  await copyTraceId.click();
  await expect(copyTraceId).toHaveAttribute('title', '已复制');
  if (process.env['VISUAL_REVIEW']) {
    await page.screenshot({ path: testInfo.outputPath('audit-logs.png'), fullPage: true });
  }
  if (await mobileMenu.isVisible()) {
    await mobileMenu.click();
  }
  await page.getByRole('link', { name: '部门管理' }).click();
  await expect(page.getByRole('heading', { name: '部门管理' })).toBeVisible();
  await expect(page.getByRole('searchbox', { name: '搜索部门名称或编码' })).toBeVisible();
  await expect(page.getByRole('combobox', { name: '按状态筛选部门' })).toBeVisible();
  await expect(page.getByRole('table', { name: '组织部门列表' })).toBeVisible();
  await expect(page.getByRole('region', { name: '部门概览' })).toContainText('部门总数');
  if (process.env['VISUAL_REVIEW']) {
    await settlePageAnimation();
    await page.screenshot({ path: testInfo.outputPath('departments.png'), fullPage: true });
  }
  if (await mobileMenu.isVisible()) {
    await mobileMenu.click();
  }
  await userNavigationGroup.click();
  await page.getByRole('link', { name: '用户列表' }).click();
  await expect(page.getByRole('heading', { name: '用户管理' })).toBeVisible();
  await expect(page.getByRole('textbox', { name: '按用户名或邮箱搜索用户' })).toBeVisible();
  await expect(page.getByRole('combobox', { name: '按角色筛选用户' })).toBeVisible();
  await expect(page.getByRole('combobox', { name: '按创建时间或姓名排序用户' })).toBeVisible();
  await expect(page.getByRole('combobox', { name: '按状态筛选用户' })).toBeVisible();
  await expect(page.getByRole('table', { name: '系统用户列表' })).toBeVisible();
  await expect(page.getByRole('checkbox', { name: '选择当前页全部可操作用户' })).toBeVisible();
  const administratorRow = page.getByRole('row').filter({ hasText: '管理员' });
  await expect(administratorRow).toContainText('2026-08-01 08:00');
  await expect(administratorRow.getByRole('button', { name: '停用用户 管理员' })).toHaveCount(0);
  await expect(administratorRow.getByRole('button', { name: '删除用户 管理员' })).toHaveCount(0);
  await expect(administratorRow.getByRole('button', { name: '编辑用户 管理员' })).toBeVisible();
  await expect(administratorRow.getByRole('button', { name: '重置用户密码 管理员' })).toBeVisible();
  await page.getByRole('button', { name: '新增用户' }).click();

  const dialog = page.getByRole('dialog');
  const editor = dialog.locator('.editor-dialog');
  await expect(dialog).toBeVisible();
  await dialog.evaluate(async (element) => {
    await Promise.allSettled(
      element.getAnimations({ subtree: true }).map((animation) => animation.finished),
    );
  });
  const dialogBox = await dialog.boundingBox();
  const viewport = page.viewportSize();
  expect(dialogBox).not.toBeNull();
  expect(viewport).not.toBeNull();
  expect(dialogBox!.width).toBeLessThanOrEqual(viewport!.width - 16);
  expect(dialogBox!.height).toBeLessThanOrEqual(viewport!.height - 16);
  await expect
    .poll(() => editor.evaluate((element) => element.scrollWidth <= element.clientWidth))
    .toBe(true);
  if (viewport!.width >= 800) {
    expect(dialogBox!.width).toBeGreaterThanOrEqual(720);
  }

  if (process.env['VISUAL_REVIEW']) {
    await page.screenshot({ path: testInfo.outputPath('add-user-dialog.png'), fullPage: true });
  }

  await dialog.getByLabel('用户名', { exact: true }).fill('new_user');
  await dialog.getByLabel('密码', { exact: true }).fill('new-user-password');
  await dialog.getByLabel('显示名称', { exact: true }).fill('新用户');
  await dialog.getByLabel('邮箱', { exact: true }).fill('new.user@example.test');
  await dialog.getByLabel('角色', { exact: true }).selectOption({ label: '查看者' });
  await dialog.getByRole('button', { name: '保存用户' }).click();
  await completeStepUp('敏感操作需要再认证');

  await expect(page.getByText('新用户', { exact: true })).toBeVisible();
  expect(createdRequest).toMatchObject({
    username: 'new_user',
    displayName: '新用户',
    roleIds: [2],
  });
  const snackBar = page.locator('mat-snack-bar-container');
  await expect(snackBar).toBeVisible();
  await snackBar.evaluate(async (element) => {
    await Promise.allSettled(
      element.getAnimations({ subtree: true }).map(({ finished }) => finished),
    );
  });
  await expect
    .poll(() =>
      snackBar
        .locator('.mdc-snackbar__surface')
        .evaluate((element) => getComputedStyle(element).backgroundColor),
    )
    .not.toBe('rgba(0, 0, 0, 0)');
  const snackBarBox = await snackBar.boundingBox();
  expect(snackBarBox).not.toBeNull();
  expect(snackBarBox!.y).toBeGreaterThanOrEqual(64);
  expect(snackBarBox!.width).toBeLessThanOrEqual(viewport!.width - 16);
  expect(snackBarBox!.x + snackBarBox!.width / 2).toBeCloseTo(viewport!.width / 2, 0);

  if (process.env['VISUAL_REVIEW']) {
    await page.screenshot({ path: testInfo.outputPath('users-snackbar.png'), fullPage: true });
  }

  const closeSnackBar = page.getByRole('button', { name: '关闭' });
  if (await closeSnackBar.isVisible()) {
    await closeSnackBar.click();
    await expect(snackBar).toBeHidden();
  }

  const createdUserRow = page.getByRole('row').filter({ hasText: '新用户' });
  await createdUserRow.getByRole('button', { name: '停用用户 新用户' }).click();
  const deactivateDialog = page.getByRole('dialog');
  await expect(deactivateDialog.getByRole('heading', { name: '停用用户' })).toBeVisible();
  await deactivateDialog.evaluate(async (element) => {
    await Promise.allSettled(
      element.getAnimations({ subtree: true }).map((animation) => animation.finished),
    );
  });
  if (process.env['VISUAL_REVIEW']) {
    await page.screenshot({ path: testInfo.outputPath('deactivate-user-dialog.png') });
  }
  await deactivateDialog.getByRole('button', { name: '停用用户' }).click();
  await completeStepUp('敏感操作需要再认证');
  await expect(createdUserRow.getByText('停用', { exact: true })).toBeVisible();
  expect(statusUpdateRequest).toEqual({ status: 'inactive' });
  await expect(createdUserRow.getByRole('button', { name: '启用用户 新用户' })).toBeVisible();

  const statusSnackBar = page.locator('mat-snack-bar-container');
  await expect(statusSnackBar).toContainText('已停用 新用户');
  await statusSnackBar.getByRole('button', { name: '关闭' }).click();
  await expect(statusSnackBar).toBeHidden();

  const rootToken = (name: string) =>
    page.evaluate((tokenName) => {
      return getComputedStyle(document.documentElement).getPropertyValue(tokenName).trim();
    }, name);

  await expect.poll(() => rootToken('--ui-color-surface-page')).toBe('#f6f8fc');
  await expect
    .poll(() =>
      page.evaluate(
        () => document.documentElement.scrollWidth <= document.documentElement.clientWidth,
      ),
    )
    .toBe(true);

  if (process.env['VISUAL_REVIEW']) {
    await page.screenshot({ path: testInfo.outputPath('users-light.png'), fullPage: true });
  }

  await page.getByRole('button', { name: '切换到暗色模式' }).click();
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
  await expect
    .poll(() => page.evaluate(() => localStorage.getItem('stock-analysis-theme')))
    .toBe('dark');
  await expect.poll(() => rootToken('--ui-color-surface-page')).toBe('#141414');

  if (process.env['VISUAL_REVIEW']) {
    await page.screenshot({ path: testInfo.outputPath('users-dark.png'), fullPage: true });
  }

  await page.getByRole('button', { name: '切换到亮色模式' }).click();
  await expect.poll(() => rootToken('--ui-color-surface-page')).toBe('#f6f8fc');
  if (await mobileMenu.isVisible()) {
    await mobileMenu.click();
  }
  await page.getByRole('link', { name: '角色管理' }).click();
  await expect(page.getByRole('heading', { name: '角色管理' })).toBeVisible();

  const gridViewButton = page.getByRole('button', { name: '卡片视图' });
  const listViewButton = page.getByRole('button', { name: '列表视图' });
  await expect(gridViewButton).toHaveAttribute('aria-pressed', 'true');
  await expect(listViewButton).toHaveAttribute('aria-pressed', 'false');

  const viewerCard = page.locator('.role-card').filter({ hasText: '查看者' });
  const superAdminCard = page.locator('.role-card').filter({ hasText: '超级管理员' });
  await expect(viewerCard.getByText('启用', { exact: true })).toBeVisible();
  await expect(viewerCard).toContainText('1 名成员');
  await expect(superAdminCard.getByRole('button', { name: '停用角色 超级管理员' })).toHaveCount(0);

  await viewerCard.getByRole('button', { name: '停用角色 查看者' }).click();
  const deactivateRoleDialog = page.getByRole('dialog');
  await expect(deactivateRoleDialog.getByRole('heading', { name: '停用角色' })).toBeVisible();
  await expect(deactivateRoleDialog).toContainText('1 名成员');
  await expect(deactivateRoleDialog).toContainText('成员将立即失去该角色授予的权限');
  await expect(deactivateRoleDialog).toContainText('可能立即失去后续管理权限');
  await deactivateRoleDialog.evaluate(async (element) => {
    await Promise.allSettled(
      element.getAnimations({ subtree: true }).map((animation) => animation.finished),
    );
  });
  if (process.env['VISUAL_REVIEW']) {
    await page.screenshot({
      path: testInfo.outputPath('deactivate-role-dialog.png'),
      fullPage: true,
    });
  }
  await deactivateRoleDialog.getByRole('button', { name: '停用角色' }).click();
  await completeStepUp('敏感操作需要再认证');
  await expect(deactivateRoleDialog).toBeHidden();
  await expect(viewerCard.getByText('停用', { exact: true })).toBeVisible();
  expect(roleStatusUpdateRequest).toEqual({ isActive: false });

  const roleSnackBar = page.locator('mat-snack-bar-container');
  await expect(roleSnackBar).toContainText('已停用 查看者');
  await roleSnackBar.getByRole('button', { name: '关闭' }).click();
  await expect(roleSnackBar).toBeHidden();
  if (process.env['VISUAL_REVIEW']) {
    await page.screenshot({ path: testInfo.outputPath('roles-grid-inactive.png'), fullPage: true });
  }

  await listViewButton.click();
  await expect(gridViewButton).toHaveAttribute('aria-pressed', 'false');
  await expect(listViewButton).toHaveAttribute('aria-pressed', 'true');

  const roleTable = page.locator('.roles-table');
  const roleTableScroll = page.locator('.role-table-scroll');
  await expect(roleTable).toBeVisible();
  await expect(roleTable.getByRole('columnheader', { name: '状态' })).toBeVisible();
  const viewerRow = roleTable.getByRole('row').filter({ hasText: '查看者' });
  await expect(viewerRow).toContainText('viewer');
  await expect(viewerRow.getByText('停用', { exact: true })).toBeVisible();
  await viewerRow.getByRole('button', { name: '启用角色 查看者' }).click();
  const activateRoleDialog = page.getByRole('dialog');
  await expect(activateRoleDialog.getByRole('heading', { name: '启用角色' })).toBeVisible();
  await expect(activateRoleDialog).toContainText('1 名成员将重新获得该角色授予的权限');
  await activateRoleDialog.getByRole('button', { name: '启用角色' }).click();
  await completeStepUp('敏感操作需要再认证');
  await expect(activateRoleDialog).toBeHidden();
  await expect(viewerRow.getByText('启用', { exact: true })).toBeVisible();
  expect(roleStatusUpdateRequest).toEqual({ isActive: true });

  await expect(roleSnackBar).toContainText('已启用 查看者');
  await roleSnackBar.getByRole('button', { name: '关闭' }).click();
  await expect(roleSnackBar).toBeHidden();
  const superAdminRow = roleTable.getByRole('row').filter({ hasText: '超级管理员' });
  await expect(superAdminRow.getByRole('button', { name: '停用角色 超级管理员' })).toHaveCount(0);
  const editViewer = viewerRow.getByRole('button', { name: '编辑角色 查看者' });
  await expect(editViewer).toBeVisible();
  await expect
    .poll(() =>
      page.evaluate(
        () => document.documentElement.scrollWidth <= document.documentElement.clientWidth,
      ),
    )
    .toBe(true);
  if (viewport!.width < 800) {
    await expect
      .poll(() => roleTableScroll.evaluate((element) => element.scrollWidth > element.clientWidth))
      .toBe(true);
  }

  if (process.env['VISUAL_REVIEW']) {
    await expect(page.getByText('已创建 新用户', { exact: true })).toBeHidden();
    await page.screenshot({ path: testInfo.outputPath('roles-list-light.png'), fullPage: true });
  }

  await editViewer.click();
  const roleDialog = page.getByRole('dialog');
  await expect(roleDialog.getByRole('heading', { name: '编辑角色' })).toBeVisible();
  const iconSelect = roleDialog.getByRole('combobox', { name: '角色图标' });
  await expect(iconSelect).toContainText('只读');
  await iconSelect.click();
  await expect(page.getByRole('option', { name: '管理员', exact: true })).toBeVisible();
  if (process.env['VISUAL_REVIEW']) {
    await page.screenshot({ path: testInfo.outputPath('role-icon-picker.png'), fullPage: true });
  }
  await page.getByRole('option', { name: '审核', exact: true }).click();
  await expect(iconSelect).toContainText('审核');
  await roleDialog.getByRole('button', { name: '取消' }).click();

  await page.goto('/role-permissions');
  await expect(page.getByRole('heading', { name: '权限分配' })).toBeVisible();
  await expect(page.getByRole('table', { name: '角色权限分配列表' })).toBeVisible();
  await expect(page.getByRole('button', { name: '编辑权限 查看者' })).toBeVisible();
  if (process.env['VISUAL_REVIEW']) {
    await settlePageAnimation();
    await page.screenshot({ path: testInfo.outputPath('role-permissions.png'), fullPage: true });
  }

  await page.getByRole('button', { name: '账户菜单' }).click();
  await page.getByRole('menuitem', { name: '账号安全' }).click();
  await expect(page.getByRole('heading', { name: '账号安全' })).toBeVisible();
  await expect(page.getByRole('heading', { name: '身份验证器已启用' })).toBeVisible();
  await expect(page.getByRole('heading', { name: '通行密钥', exact: true })).toBeVisible();
  await expect(page.getByRole('heading', { name: '恢复码', exact: true })).toBeVisible();
  if (process.env['VISUAL_REVIEW']) {
    await settlePageAnimation();
    await page.screenshot({ path: testInfo.outputPath('security.png'), fullPage: true });
  }
});

test('redirects an unauthenticated deep link to login', async ({ page }) => {
  await page.goto('/users');
  await expect(page).toHaveURL(/\/login$/);
  await expect(page.getByRole('button', { name: '登录' })).toBeVisible();
});

test('renders the public error page with one main landmark', async ({ page }, testInfo) => {
  await page.goto('/404');
  await expect(page.getByRole('heading', { name: '页面不存在' })).toBeVisible();
  await expect(page.locator('main')).toHaveCount(1);
  await expect(page.getByRole('button', { name: '返回首页' })).toBeVisible();
  await expect(page.getByRole('button', { name: '返回上一页' })).toBeVisible();
  if (process.env['VISUAL_REVIEW']) {
    await page.locator('.error-content').evaluate(async (element) => {
      await Promise.allSettled(
        element.getAnimations({ subtree: true }).map((animation) => animation.finished),
      );
    });
    await page.screenshot({ path: testInfo.outputPath('error-404.png'), fullPage: true });
  }
});
