import { describe, expect, it } from 'vitest';

import { ANONYMOUS, isAuthenticated, tenantId } from '../src/index';

describe('@base/auth', () => {
  it('匿名态不算已登录', () => {
    expect(isAuthenticated(ANONYMOUS)).toBe(false);
    expect(tenantId(ANONYMOUS)).toBeUndefined();
  });

  it('已登录且有用户 ID 时才成立', () => {
    expect(isAuthenticated({ authenticated: true, userId: 'u-1' })).toBe(true);
    expect(isAuthenticated({ authenticated: true, userId: '' })).toBe(false);
    expect(isAuthenticated({ authenticated: true })).toBe(false);
  });

  it('已登录时返回租户 ID', () => {
    const state = { authenticated: true, userId: 'u-1', tenantId: 'tenant-a' };
    expect(tenantId(state)).toBe('tenant-a');
  });
});
