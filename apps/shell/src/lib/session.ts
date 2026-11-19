/**
 * 会话持久化（D99 / AC-S-068 补充）。
 *
 * 会话（access / refresh token + **租户**）存 `sessionStorage`：
 * 整页重载后恢复 → 直接敲受保护 URL 时守卫判成「已登录但无权限」→ 落 **403 页**。
 * `sessionStorage` 的作用域是「单标签页 + 单次会话」—— 新开标签页**不**继承会话。
 *
 * # 为什么不存权限点
 *
 * 权限点**不**持久化：启动时从 `GET /api/rbac/me/permissions` **现取**。
 * 存下来的权限点在授权被撤销后仍是旧值，会变成「前端说能点、后端 403」。
 *
 * # 存储不可用时降级
 *
 * 隐私模式 / 非浏览器环境里 `sessionStorage` 可能不存在或抛错：
 * 退回**内存模式**（功能可用，但整页重载会丢会话），并留一条可发现的告警 ——
 * 不抛未捕获异常。
 *
 * # 为什么不存 `localStorage`
 *
 * refresh token 长期驻留浏览器是 XSS 的放大器；`sessionStorage` 的泄露窗口小得多。
 */

/** 存储键（带版本号：结构变化时旧结构不会被当成新结构读）。 */
export const SESSION_STORAGE_KEY = 'base.shell.session.v1';

/** 最小键值存储（`sessionStorage` 的结构子集；单测可以注入假件）。 */
export interface KeyValueStorage {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
  removeItem(key: string): void;
}

/** 会话内容（令牌 + 租户；租户必须跟会话一起走，否则重载后请求又缺头）。 */
export interface SessionState {
  /** access token。 */
  readonly accessToken?: string;
  /** refresh token。 */
  readonly refreshToken?: string;
  /** 租户标识（登录页填的，登录后整个会话沿用）。 */
  readonly tenantId?: string;
}

/** 会话存储。 */
export interface SessionStore {
  /** 当前会话（内存中的权威值）。 */
  readonly state: SessionState;
  /** 是否已降级为「仅内存」（`sessionStorage` 不可用或写入失败）。 */
  readonly persistent: boolean;
  /** 设置租户（登录请求要用它；随下一次 `setTokens` 一起持久化）。 */
  setTenant(tenantId: string): void;
  /** 登录 / 刷新成功后写入令牌（租户沿用当前值）。 */
  setTokens(tokens: { readonly accessToken: string; readonly refreshToken?: string }): void;
  /** 清空会话（登出 / 会话失效）。 */
  clear(): void;
}

/** 读写都不让异常逃逸 —— 存储只是「锦上添花」，不能影响页面可用性。 */
function readRaw(storage: KeyValueStorage): string | null {
  try {
    return storage.getItem(SESSION_STORAGE_KEY);
  } catch {
    return null;
  }
}

/** 解析存储内容；形状不符（旧结构 / 手工篡改）→ `undefined`。 */
function parseStored(raw: string | null): SessionState | undefined {
  if (raw === null) {
    return undefined;
  }
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch {
    return undefined;
  }
  if (typeof parsed !== 'object' || parsed === null) {
    return undefined;
  }
  const candidate = parsed as {
    readonly accessToken?: unknown;
    readonly refreshToken?: unknown;
    readonly tenantId?: unknown;
  };
  if (typeof candidate.accessToken !== 'string' || candidate.accessToken === '') {
    return undefined;
  }
  // 没有租户的会话**不可用**：恢复后每个请求都会缺 `x-tenant-id`（必 401）。
  if (typeof candidate.tenantId !== 'string' || candidate.tenantId === '') {
    return undefined;
  }
  const refreshToken =
    typeof candidate.refreshToken === 'string' && candidate.refreshToken !== ''
      ? candidate.refreshToken
      : undefined;
  return { accessToken: candidate.accessToken, refreshToken, tenantId: candidate.tenantId };
}

/** 探测 `sessionStorage` 是否可用（访问与写入都可能抛错）。 */
function detectStorage(): KeyValueStorage | null {
  try {
    const storage = (globalThis as { sessionStorage?: KeyValueStorage }).sessionStorage;
    if (storage === undefined) {
      return null;
    }
    // 隐私模式下 `getItem` 可能正常、写入才抛错 —— 用一次真实写入证明可用。
    const probe = `${SESSION_STORAGE_KEY}.probe`;
    storage.setItem(probe, '1');
    storage.removeItem(probe);
    return storage;
  } catch (error) {
    // eslint-disable-next-line no-console -- 降级必须留痕，否则「重载掉登录态」无从排查
    console.warn('[session] sessionStorage 不可用，退回内存模式（整页重载会丢失会话）', error);
    return null;
  }
}

/**
 * 构造会话存储。
 *
 * @param storage `undefined` = 自动探测 `sessionStorage`；`null` = 仅内存（单测用）
 */
export function createSessionStore(storage?: KeyValueStorage | null): SessionStore {
  let target = storage === undefined ? detectStorage() : storage;
  let state: SessionState = {};

  if (target !== null) {
    const raw = readRaw(target);
    const restored = parseStored(raw);
    if (restored !== undefined) {
      state = restored;
    } else if (raw !== null) {
      // 旧结构 / 损坏内容：直接清掉，免得每次启动都读到一个不可用的会话。
      try {
        target.removeItem(SESSION_STORAGE_KEY);
      } catch {
        // 清不掉也不影响：内存里已经是空会话。
      }
    }
  }

  const persist = (): void => {
    if (target === null) {
      return;
    }
    if (state.accessToken === undefined || state.tenantId === undefined) {
      return;
    }
    try {
      target.setItem(
        SESSION_STORAGE_KEY,
        JSON.stringify({
          accessToken: state.accessToken,
          refreshToken: state.refreshToken,
          tenantId: state.tenantId,
        }),
      );
    } catch (error) {
      // 配额 / 隐私模式的写入失败：降级为仅内存，当前页面继续可用。
      target = null;
      // eslint-disable-next-line no-console -- 降级必须留痕
      console.warn('[session] 写入 sessionStorage 失败，退回内存模式', error);
    }
  };

  return {
    get state(): SessionState {
      return state;
    },
    get persistent(): boolean {
      return target !== null;
    },
    setTenant(tenantId: string): void {
      state = { ...state, tenantId };
      persist();
    },
    setTokens(tokens): void {
      state = {
        ...state,
        accessToken: tokens.accessToken,
        refreshToken: tokens.refreshToken ?? state.refreshToken,
      };
      persist();
    },
    clear(): void {
      state = {};
      if (target === null) {
        return;
      }
      try {
        target.removeItem(SESSION_STORAGE_KEY);
      } catch {
        // 删不掉也不影响：内存里已经是空会话。
      }
    },
  };
}
