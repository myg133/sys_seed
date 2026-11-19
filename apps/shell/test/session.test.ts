import { describe, expect, it } from 'vitest';

import { SESSION_STORAGE_KEY, createSessionStore, type KeyValueStorage } from '../src/lib/session';

/** 内存版键值存储（等价于浏览器 `sessionStorage` 的结构）。 */
function memoryStorage(): { storage: KeyValueStorage; map: Map<string, string> } {
  const map = new Map<string, string>();
  return {
    map,
    storage: {
      getItem: (key) => map.get(key) ?? null,
      setItem: (key, value) => {
        map.set(key, value);
      },
      removeItem: (key) => {
        map.delete(key);
      },
    },
  };
}

describe('会话持久化（D99）', () => {
  it('令牌 + 租户写入后，新的 store 能恢复（等价于整页重载）', () => {
    const { storage, map } = memoryStorage();

    const first = createSessionStore(storage);
    first.setTenant('tenant-a');
    first.setTokens({ accessToken: 'access-1', refreshToken: 'refresh-1' });

    // 写入的是「令牌 + 租户」—— 租户必须一起走，否则恢复后请求缺头。
    const raw = map.get(SESSION_STORAGE_KEY);
    expect(raw).toBeTypeOf('string');
    expect(JSON.parse(raw ?? '{}')).toEqual({
      accessToken: 'access-1',
      refreshToken: 'refresh-1',
      tenantId: 'tenant-a',
    });

    const restored = createSessionStore(storage).state;
    expect(restored).toEqual({
      accessToken: 'access-1',
      refreshToken: 'refresh-1',
      tenantId: 'tenant-a',
    });
  });

  it('刷新后的新令牌也会落盘（租户沿用）', () => {
    const { storage, map } = memoryStorage();
    const store = createSessionStore(storage);
    store.setTenant('tenant-a');
    store.setTokens({ accessToken: 'access-1', refreshToken: 'refresh-1' });
    store.setTokens({ accessToken: 'access-2', refreshToken: 'refresh-2' });

    expect(JSON.parse(map.get(SESSION_STORAGE_KEY) ?? '{}')).toEqual({
      accessToken: 'access-2',
      refreshToken: 'refresh-2',
      tenantId: 'tenant-a',
    });
  });

  it('缺租户的会话**不**恢复（恢复后每个请求都会缺 x-tenant-id）', () => {
    const { storage } = memoryStorage();
    storage.setItem(SESSION_STORAGE_KEY, JSON.stringify({ accessToken: 'access-1' }));

    expect(createSessionStore(storage).state).toEqual({});
  });

  it('损坏 / 旧结构的存储内容不抛错，且被清理', () => {
    const { storage, map } = memoryStorage();
    storage.setItem(SESSION_STORAGE_KEY, '{not json');

    expect(createSessionStore(storage).state).toEqual({});
    expect(map.has(SESSION_STORAGE_KEY)).toBe(false);
  });

  it('存储不可用时降级为内存模式（不抛未捕获异常）', () => {
    const store = createSessionStore(null);
    expect(store.persistent).toBe(false);
    expect(() => {
      store.setTenant('tenant-a');
      store.setTokens({ accessToken: 'access-1', refreshToken: 'refresh-1' });
    }).not.toThrow();
    expect(store.state.accessToken).toBe('access-1');
    expect(store.state.tenantId).toBe('tenant-a');
  });

  it('读取抛错的存储也不会让构造函数失败', () => {
    const broken: KeyValueStorage = {
      getItem: () => {
        throw new Error('denied');
      },
      setItem: () => {
        throw new Error('denied');
      },
      removeItem: () => {
        throw new Error('denied');
      },
    };

    const store = createSessionStore(broken);
    expect(store.state).toEqual({});
    expect(() => store.setTokens({ accessToken: 'access-1' })).not.toThrow();
    expect(store.state.accessToken).toBe('access-1');
  });

  it('清空会话同时清掉持久化内容', () => {
    const { storage, map } = memoryStorage();
    const store = createSessionStore(storage);
    store.setTenant('tenant-a');
    store.setTokens({ accessToken: 'access-1', refreshToken: 'refresh-1' });

    store.clear();

    expect(store.state).toEqual({});
    expect(map.has(SESSION_STORAGE_KEY)).toBe(false);
    expect(createSessionStore(storage).state).toEqual({});
  });

  it('不传存储时**默认**用 globalThis.sessionStorage（两次构造之间会话保留）', () => {
    const { storage } = memoryStorage();
    const original = Object.getOwnPropertyDescriptor(globalThis, 'sessionStorage');
    Object.defineProperty(globalThis, 'sessionStorage', { value: storage, configurable: true });
    try {
      const store = createSessionStore();
      expect(store.persistent).toBe(true);
      store.setTenant('tenant-a');
      store.setTokens({ accessToken: 'access-1', refreshToken: 'refresh-1' });

      // 同一份 sessionStorage 上再构造 = 整页重载后的恢复路径。
      expect(createSessionStore().state).toEqual({
        accessToken: 'access-1',
        refreshToken: 'refresh-1',
        tenantId: 'tenant-a',
      });
    } finally {
      if (original === undefined) {
        delete (globalThis as { sessionStorage?: unknown }).sessionStorage;
      } else {
        Object.defineProperty(globalThis, 'sessionStorage', original);
      }
    }
  });
});
