/**
 * 前端 `can()` 与后端 `require_permission` 的判定**必须一致**（AC-S-066 步骤 5）。
 *
 * # 这份金标准是哪来的
 *
 * **真机实测**（2026-11-19，真实 `base serve` 进程 + 应用角色连接 + `base_test` 库），
 * 不是从实现里抄的期望值。三个夹具用户由 `crates/platform-rbac/tests/me_permissions.rs`
 * 建立：
 *
 * | 用户 | `/api/rbac/me/permissions` | `GET /api/rbac/roles`（需 `rbac:role:read`） |
 * |---|---|---|
 * | `u-perm-read` | `["rbac:role:read"]` | **200** |
 * | `u-perm-write` | `["rbac:role:read","rbac:role:write"]` | **200** |
 * | `u-perm-none` | `[]` | **403** |
 *
 * 前端拿**同一份**权限点集合调 `can()` → 必须与后端的 HTTP 状态一一对应。
 * 两边不一致 = 用户会看到「按钮能点，但点了报 403」。
 */
import { describe, expect, it } from 'vitest';

import { can } from '../src/index';

const GOLDEN: Array<{ user: string; permissions: string[]; backendStatus: number }> = [
  { user: 'u-perm-read', permissions: ['rbac:role:read'], backendStatus: 200 },
  { user: 'u-perm-write', permissions: ['rbac:role:read', 'rbac:role:write'], backendStatus: 200 },
  { user: 'u-perm-none', permissions: [], backendStatus: 403 },
];

describe('AC-S-066 步骤 5：前后端判定一致（真机金标准）', () => {
  it('can() 的判定与后端实测的 HTTP 状态一一对应', () => {
    for (const item of GOLDEN) {
      const state = {
        status: 'authenticated' as const,
        authenticated: true,
        userId: item.user,
        tenantId: 'tenant-a',
        permissions: item.permissions,
      };
      const frontendAllowed = can(state, 'rbac:role:read');
      const backendAllowed = item.backendStatus === 200;
      expect(
        frontendAllowed,
        `${item.user}: 前端 can()=${frontendAllowed}，后端 HTTP=${item.backendStatus}`
      ).toBe(backendAllowed);
    }
  });

  it('未登记权限点一律 false（不抛错）', () => {
    const state = {
      status: 'authenticated' as const,
      authenticated: true,
      userId: 'u-perm-read',
      tenantId: 'tenant-a',
      permissions: ['rbac:role:read'],
    };
    expect(can(state, 'order:invoice:delete')).toBe(false);
  });

  it('空权限点集合 → 一律 false（fail closed）', () => {
    const state = {
      status: 'authenticated' as const,
      authenticated: true,
      userId: 'u-perm-none',
      tenantId: 'tenant-a',
      permissions: [],
    };
    expect(can(state, 'rbac:role:read')).toBe(false);
  });
});
