/**
 * @base/auth —— 登录态与权限（REQ-011 / AC-S-066 ~ AC-S-068）。
 *
 * # 三条语义
 *
 * | 能力 | 约定 |
 * |---|---|
 * | `can(permission)` | **纯集合判定**：集合为空 → `false`（fail closed）；没有 → `false`；未登记 → `false` |
 * | `guard(permission)` | 未登录 → 落登录页；已登录无权限 → 落 403 页；有权限 → 放行 |
 * | 条件渲染 | 无权限时**不渲染** children（不是 `display:none` —— 内容仍在 DOM 里） |
 *
 * # 为什么不做通配匹配
 *
 * 通配在后端展开（`RbacService::effective_permissions`），
 * 前端拿到的已经是展开后的权限点集合 —— 两边用同一套数据，判定才不会分叉。
 * 前端再做一次通配展开，只会让「前端说有、后端 403」的风险回来。
 *
 * # 为什么用结构化类型接客户端
 *
 * 本包**不 import** `@base/sdk`：包的构建链只有 `tsc`（没有 project reference），
 * 跨包引用类型会让 `pnpm -r check` 在「还没构建」时解析不到（CI 的 lint job 是全新 checkout）。
 * `AuthApiClient` 与 SDK 的 `ApiClient` 结构兼容 —— 宿主把真客户端传进来即可。
 */

/** 当前用户。 */
export interface AuthUser {
  /** 用户 ID。 */
  readonly id: string;
  /** 登录名。 */
  readonly username: string;
  /** 归属租户。 */
  readonly tenantId?: string;
}

/** 登录态。 */
export interface AuthState {
  /** 登录态：`'authenticated'` 表示已登录。 */
  readonly status: 'anonymous' | 'authenticated';
  /** 与 `status` 等价的布尔形式（既有调用方用这个字段）。 */
  readonly authenticated: boolean;
  /** 当前用户 ID。 */
  readonly userId?: string;
  /** 当前租户 ID。 */
  readonly tenantId?: string;
  /**
   * 当前用户的有效权限点集合（**升序、去重**）。
   *
   * 空集合 = **没有任何权限**（fail closed）——
   * 「还没加载完」与「没有权限」都判 `false`。
   */
  readonly permissions: readonly string[];
}

/** 未登录态。 */
export const ANONYMOUS: AuthState = Object.freeze({
  status: 'anonymous',
  authenticated: false,
  permissions: Object.freeze([]) as readonly string[],
});

/** 取登录态的规范形态（兼容只有 `authenticated` 的历史入参）。 */
function statusOf(state: AuthState): 'anonymous' | 'authenticated' {
  const raw = (state as { status?: string }).status;
  if (raw === 'authenticated' || raw === 'anonymous') {
    return raw;
  }
  return state.authenticated === true ? 'authenticated' : 'anonymous';
}

/** 是否已登录。 */
export function isAuthenticated(state: AuthState): boolean {
  return statusOf(state) === 'authenticated';
}

/** 取当前租户 ID，未登录或未绑定租户时为 `undefined`。 */
export function tenantId(state: AuthState): string | undefined {
  return isAuthenticated(state) ? state.tenantId : undefined;
}

/**
 * 判定当前用户是否拥有某个权限点。
 *
 * - 权限点集合为空 → `false`（fail closed：没有集合 ≠ 全部权限）
 * - 集合里没有该权限点 → `false`
 * - 未登记的权限点 → `false`（不抛错）
 */
export function can(state: AuthState, permission: string): boolean {
  const permissions = state.permissions ?? [];
  if (permissions.length === 0) {
    return false;
  }
  return permissions.includes(permission);
}

/** 守卫判定结果。 */
export interface GuardResult {
  /** 是否放行。 */
  readonly allowed: boolean;
  /** 拒绝时的落点（登录页 / 403 页）；放行时为 `undefined`。 */
  readonly redirectTo?: string;
}

/** 守卫选项。 */
export interface GuardOptions {
  /** 未认证时跳哪（默认 `/login`）。 */
  readonly loginPath?: string;
  /** 拒绝时跳哪（默认 `/403`）。 */
  readonly forbiddenPath?: string;
}

/**
 * 路由守卫的**纯判定**（框架无关）。
 *
 * - 未登录 → `{ allowed: false, redirectTo: '/login' }`（**不是** 403 —— 未认证与无权限要分清）
 * - 已登录但无权限 → `{ allowed: false, redirectTo: '/403' }`
 * - 已登录且有权限 → `{ allowed: true }`
 */
export function guard(
  state: AuthState,
  permission: string,
  options: GuardOptions = {},
): GuardResult {
  const loginPath = options.loginPath ?? '/login';
  const forbiddenPath = options.forbiddenPath ?? '/403';
  if (!isAuthenticated(state)) {
    return { allowed: false, redirectTo: loginPath };
  }
  if (!can(state, permission)) {
    return { allowed: false, redirectTo: forbiddenPath };
  }
  return { allowed: true };
}

/** 条件渲染的判定结果（框架无关）。 */
export interface ConditionalRender {
  /** 是否渲染 children。 */
  readonly renderChildren: boolean;
  /** 无权限时的兜底内容；`undefined` 表示**什么都不渲染**。 */
  readonly fallback?: string;
}

/**
 * 条件渲染辅助。
 *
 * 无权限时**不渲染** children —— 不是 `display:none`（那样内容还在 DOM 里）。
 */
export function conditionalRender(
  state: AuthState,
  permission: string,
  fallback?: string,
): ConditionalRender {
  const allowed = can(state, permission);
  return { renderChildren: allowed, fallback: allowed ? undefined : fallback };
}

/**
 * `<Can>` 视图层的 props 契约（**类型-only**）。
 *
 * # 为什么类型住在包里、组件住在应用里
 *
 * 包的构建是 `tsc`（`.svelte` 文件进不了产物），所以 `@base/auth` **不能**产出
 * Svelte 组件；但**类型契约**可以、也应该由包提供 ——
 * 否则每个宿主应用都要各写一份 props 接口，**类型安全就散掉了**。
 *
 * # 为什么 `children` 是泛型
 *
 * `children` 的类型由**宿主框架**决定（Svelte 的 `Snippet`、React 的 `ReactNode` …），
 * 而 `@base/auth` 必须**保持框架无关**（它只做判定）。
 * 所以这里用**泛型参数** `Children`：
 * 宿主应用写 `CanComponentProps<Snippet>` 时，
 * `children` 就是 `Snippet` → `{@render props.children?.()}` 可被类型检查。
 *
 * 默认 `unknown` 时 `children` **不可调用** —— 这正是我们想要的：
 * 宿主**必须**显式给出子内容的类型，否则 `@render` 无法通过类型检查。
 */
export interface CanComponentProps<Children = unknown> {
  /** 要求的权限点编码。 */
  readonly permission: string;
  /** 当前登录态；不传时从宿主的上下文取。 */
  readonly state?: AuthState;
  /** 有权限时渲染的内容（类型由宿主框架给出）。 */
  readonly children?: Children;
  /** 无权限时的兜底文案；不传则**什么都不渲染**。 */
  readonly fallback?: string;
}

/** 登录会话（令牌由宿主保管）。 */
export interface AuthSession {
  /** access token。 */
  readonly accessToken: string;
  /** refresh token。 */
  readonly refreshToken: string;
  /** 当前用户。 */
  readonly user: AuthUser;
  /** 有效权限点集合。 */
  readonly permissions: readonly string[];
}

/**
 * 认证请求的额外选项（与 `@base/sdk` 的 `RequestOptionsWithoutTarget` 结构兼容）。
 *
 * 只声明本包用到的字段：认证端点必须跳过「401 自动刷新」（D100）。
 */
export interface AuthRequestOptions {
  /** 见 `@base/sdk` 的 `RequestOptions.skipAuthRetry`。 */
  readonly skipAuthRetry?: boolean;
}

/**
 * 认证所需的最小客户端能力。
 *
 * 与 `@base/sdk` 的 `ApiClient` **结构兼容**（宿主把真客户端传进来即可）。
 */
export interface AuthApiClient {
  /** POST 请求。 */
  post<T = unknown>(path: string, body?: unknown, options?: AuthRequestOptions): Promise<T>;
  /** GET 请求。 */
  get<T = unknown>(path: string): Promise<T>;
}

/** 登录响应（后端 `LoginResponse`，camelCase）。 */
interface LoginResponse {
  readonly accessToken?: string;
  readonly refreshToken?: string;
  readonly tokenType?: string;
  readonly expiresIn?: number;
  readonly user?: {
    readonly id?: string;
    readonly username?: string;
    readonly tenantId?: string;
  };
  readonly permissions?: readonly string[];
}

/** 权限点响应（后端 `GET /api/rbac/me/permissions`）。 */
interface PermissionsResponse {
  readonly permissions?: readonly string[];
}

/** 认证存储的构造参数。 */
export interface AuthStoreOptions {
  /** 登录接口路径（默认 `/api/iam/login`）。 */
  readonly loginPath?: string;
  /** 权限点接口路径（默认 `/api/rbac/me/permissions`）。 */
  readonly permissionsPath?: string;
  /** 登录成功后的回调（令牌由宿主保管）。 */
  readonly onLogin?: (session: AuthSession) => void;
  /** 登出后的回调（宿主清理令牌）。 */
  readonly onLogout?: () => void;
}

/** 认证存储。 */
export interface AuthStore {
  /** 当前登录态。 */
  readonly state: AuthState;
  /** 登录：成功后写入权限点集合。 */
  login(username: string, password: string): Promise<void>;
  /** 登出：清空登录态与权限点集合。 */
  logout(): Promise<void>;
  /** 重新拉取权限点集合。 */
  refreshPermissions(): Promise<void>;
  /** 判定权限点。 */
  can(permission: string): boolean;
  /** 路由守卫判定。 */
  guard(permission: string, options?: GuardOptions): GuardResult;
}

/** 去重并保持顺序。 */
function dedupe(permissions: readonly string[] | undefined): readonly string[] {
  if (permissions === undefined) {
    return [];
  }
  return [...new Set(permissions)];
}

/** 构造认证存储。 */
export function createAuthStore(client: AuthApiClient, options: AuthStoreOptions = {}): AuthStore {
  const loginPath = options.loginPath ?? '/api/iam/login';
  const permissionsPath = options.permissionsPath ?? '/api/rbac/me/permissions';
  let current: AuthState = ANONYMOUS;

  return {
    get state(): AuthState {
      return current;
    },

    async login(username: string, password: string): Promise<void> {
      // ⚠️ 登录是**认证端点**：401（密码错 / 租户错…）必须原样透传后端错误码，
      //    不得触发自动刷新 —— 刷新救不了密码错，还会把 `INVALID_CREDENTIALS`
      //    覆盖成 `TOKEN_REVOKED`（D100）。
      const response = await client.post<LoginResponse>(
        loginPath,
        { username, password },
        { skipAuthRetry: true },
      );
      const accessToken = response?.accessToken;
      if (typeof accessToken !== 'string' || accessToken === '') {
        throw new Error('登录响应缺少访问令牌');
      }
      const permissions = dedupe(response?.permissions);
      const user: AuthUser = {
        id: response?.user?.id ?? '',
        username: response?.user?.username ?? username,
        tenantId: response?.user?.tenantId,
      };
      current = {
        status: 'authenticated',
        authenticated: true,
        userId: user.id,
        tenantId: user.tenantId,
        permissions,
      };
      options.onLogin?.({
        accessToken,
        refreshToken: response?.refreshToken ?? '',
        user,
        permissions,
      });
    },

    async logout(): Promise<void> {
      current = { ...ANONYMOUS, permissions: [] };
      options.onLogout?.();
    },

    async refreshPermissions(): Promise<void> {
      const response = await client.get<PermissionsResponse>(permissionsPath);
      current = { ...current, permissions: dedupe(response?.permissions) };
    },

    can(permission: string): boolean {
      return can(current, permission);
    },

    guard(permission: string, guardOptions?: GuardOptions): GuardResult {
      return guard(current, permission, guardOptions);
    },
  };
}
