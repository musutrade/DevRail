import { expect, test } from '@playwright/test';
import * as OTPAuth from 'otpauth';
import type { RoleResponse } from '../src/app/generated/api/models/role-response';

test('通过真实 Angular、Axum 和 PostgreSQL 完成登录与用户创建', async ({ page }, testInfo) => {
  const suffix = Date.now().toString(36);
  const username = `smoke_${suffix}`;
  const displayName = `全栈测试用户 ${suffix}`;
  const adminUsername = `fullstack_admin_${testInfo.retry}`;
  const adminDisplayName = `全栈测试管理员 ${testInfo.retry}`;

  await page.goto('/login');
  await page.getByLabel('用户名').fill(adminUsername);
  await page.getByLabel('密码', { exact: true }).fill('Fullstack-Smoke-Password-2026!');
  await page.getByRole('button', { name: '登录' }).click();
  await expect(page.getByRole('heading', { name: '安全验证' })).toBeVisible();
  const secret = (await page.locator('.secret-value code').textContent())?.trim();
  expect(secret).toBeTruthy();
  const totp = new OTPAuth.TOTP({
    algorithm: 'SHA1',
    digits: 6,
    period: 30,
    secret: OTPAuth.Secret.fromBase32(secret!),
  });
  await page.getByLabel('身份验证器验证码').fill(totp.generate());
  await page.getByRole('button', { name: '验证', exact: true }).click();
  await expect(page.getByRole('heading', { name: '保存恢复码' })).toBeVisible();
  await page.getByRole('button', { name: '进入系统' }).click();
  await expect(page.getByRole('heading', { name: '权限目录' })).toBeVisible();

  await page.getByRole('button', { name: '用户管理' }).click();
  await page.getByRole('link', { name: '用户列表' }).click();
  await expect(page.getByRole('heading', { name: '用户管理' })).toBeVisible();
  await expect(page.getByRole('row').filter({ hasText: adminDisplayName })).toBeVisible();

  const rolesResponse = await page.request.get('/api/v1/roles');
  expect(rolesResponse.ok()).toBeTruthy();
  const roles = (await rolesResponse.json()) as RoleResponse[];
  const viewerRole = roles.find((role) => role.code === 'viewer');
  expect(viewerRole).toBeDefined();

  await page.getByRole('button', { name: '新增用户' }).click();
  const dialog = page.getByRole('dialog');
  await dialog.getByLabel('用户名').fill(username);
  await dialog.getByLabel('密码').fill('Smoke-User-Password-2026!');
  await dialog.getByLabel('显示名称').fill(displayName);
  await dialog.getByLabel('邮箱').fill(`${username}@example.test`);
  await dialog.getByLabel('状态').selectOption('inactive');
  await expect(dialog.getByLabel('状态')).toHaveValue('inactive');
  await dialog.getByLabel('角色').selectOption({ label: viewerRole!.name });
  await expect(dialog.getByLabel('角色').locator('option:checked')).toHaveText(viewerRole!.name);
  await dialog.getByRole('button', { name: '保存用户' }).click();

  const stepUpDialog = page.getByRole('dialog');
  await expect(stepUpDialog.getByRole('heading', { name: '敏感操作需要再认证' })).toBeVisible();
  await stepUpDialog.getByLabel('当前密码').fill('Fullstack-Smoke-Password-2026!');
  await stepUpDialog.getByLabel('身份验证器验证码').fill(totp.generate());
  await stepUpDialog.getByRole('button', { name: '继续' }).click();

  await expect(page.locator('mat-snack-bar-container')).toContainText(`已创建 ${displayName}`);
  await page.getByPlaceholder('按用户名或邮箱搜索...').fill(username);
  await expect(
    page.getByRole('row').filter({ hasText: displayName }).filter({ hasText: viewerRole!.name }),
  ).toBeVisible();
  await expect(page.getByText('显示第 1-1 条，共 1 条')).toBeVisible();

  await page.getByRole('button', { name: '账户菜单' }).click();
  await page.getByRole('menuitem', { name: '退出登录' }).click();
  await expect(page).toHaveURL(/\/login$/);
});
