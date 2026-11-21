/**
 * 应用接入接口封装（REQ-016 / AC-S-156 · 160 · 161 · 162 —— 前端侧）。
 *
 * # 字段名 = camelCase（契约 C13 / D258）
 *
 * `GET /api/apps*` 的 JSON 字段**一律 camelCase**：`appId` / `clientId` /
 * `displayName` / `iconUrl` / `launchUrl` / `sortOrder` / `createdAt` / `createdBy`。
 * 逐字对照 `design-summary.md` §3.1 的字段表 —— 改一个要改两处。
 *
 * # 三类响应形状，不要混着用（`design-summary.md` §3.1）
 *
 * | 端点 | 形状 |
 * |---|---|
 * | `GET /api/apps` / `recent` / `frequent` | **数组** `[AppView]` |
 * | `GET /api/apps/workspace` | `{ groups: [{ category, apps: [AppView] }] }` |
 * | `GET /api/apps/navigation` | `{ items: [NavigationItem] }` |
 *
 * 把 `workspace` 当数组读、或把 `recent` 当 `{ items: [] }` 读，都会**静默**得到
 * 「0 条」而不是报错 —— 所以解析各自独立、且形状不对时**不抛错**（列表页不白屏）。
 *
 * # `NavigationItem` 按来源**分别定义**（D284 / S-09）
 *
 * `MenuSpec` 只有 4 个字段（`label` / `icon` / `route` / `perm`，`manifest.rs:120-133`）
 * → `module` 项**没有** `labelKey` / `category` / `sortOrder` / `key`；
 * `app` 项才有 `appId`。所以两类是**判别联合**，不是「一个都有的宽接口」。
 *
 * ⚠️ `label` 是**已渲染的中文文案**（不是翻译键）→ 前端直接渲染，**不过 `t()`**。
 * 代价：模块菜单不随语言切换（**TD-049**）。
 *
 * # `launch_url` 渲染前必须过 scheme 白名单（**安全判据**）
 *
 * `launch_url` 由租户管理员填，用户点击时是**整页跳转**（`kind="external"`，D269）。
 * 若允许 `javascript:` / `data:` 进 `href`，等于给租户管理员一个对**本租户所有用户**
 * 执行脚本的入口（AC-S-156 步骤 9 的安全判据）。
 * → `safeLaunchUrl` 只放行 `http:` / `https:`，其余一律 `undefined`（不渲染入口）。
 */

import type { ApiClient } from '@base/sdk';

/* -------------------------------------------------------------------------- */
/* 路径常量（逐字 —— 与棒 1 的实现对照用）                                      */
/* -------------------------------------------------------------------------- */

/** 应用注册表（列 / 建）。 */
export const APPS_PATH = '/api/apps';

/** 工作台（按 category 分组 + 确定性排序）。 */
export const APPS_WORKSPACE_PATH = '/api/apps/workspace';

/** 最近访问。 */
export const APPS_RECENT_PATH = '/api/apps/recent';

/** 常用应用。 */
export const APPS_FREQUENT_PATH = '/api/apps/frequent';

/** 导航项（`module` + `app` 两类来源合并，按权限点过滤）。 */
export const APPS_NAVIGATION_PATH = '/api/apps/navigation';

/** 单个应用路径（`/api/apps/{appId}`）。 */
export function appPath(appId: string): string {
  return `${APPS_PATH}/${encodeURIComponent(appId)}`;
}

/** 记一次访问（`POST /api/apps/{appId}/access`）。 */
export function appAccessPath(appId: string): string {
  return `${appPath(appId)}/access`;
}

/**
 * 把 `launch_url` 置 NULL 的**专用**端点（D283 / S-08）。
 *
 * `Option<String>` 下 `PATCH` 的 `null` 与「省略键」在 serde 里**分不开**
 * → 显式置空必须走 `DELETE`（返回 **204**）。
 */
export function appLaunchUrlPath(appId: string): string {
  return `${appPath(appId)}/launch-url`;
}

/* -------------------------------------------------------------------------- */
/* 权限点（契约 C4：`module:resource:action`）                                   */
/* -------------------------------------------------------------------------- */

/** 读注册表（`/admin/apps` 的路由守卫用它）。 */
export const APP_REGISTRY_READ_PERMISSION = 'app:registry:read';

/** 写注册表（注册 / 修改 / 清空 `launch_url`）。 */
export const APP_REGISTRY_WRITE_PERMISSION = 'app:registry:write';

/** 删注册表。 */
export const APP_REGISTRY_DELETE_PERMISSION = 'app:registry:delete';

/* -------------------------------------------------------------------------- */
/* 白名单                                                                      */
/* -------------------------------------------------------------------------- */

/** 应用状态（`platform_app.status`）。 */
export const APP_STATUSES = ['active', 'hidden'] as const;

/** 应用状态。 */
export type AppStatus = (typeof APP_STATUSES)[number];

/** 导航项的来源。 */
export const APP_NAVIGATION_SOURCES = ['module', 'app'] as const;

/** 导航项的跳转语义：`internal` 走 SPA 路由、`external` 整页跳（D269）。 */
export const APP_NAVIGATION_KINDS = ['internal', 'external'] as const;

/**
 * 可写字段白名单（`POST` / `PATCH` 的请求体只发这些键）。
 *
 * `appId` / `status` / `scopes` / `createdAt` / `createdBy` 是**响应侧**字段：
 * `appId` 由后端生成、`scopes` 是 client `allowed_scopes` 的**只读投影**（D266）
 * → 前端**不**发它们（发了也会被后端忽略，但白名单让「误发」不可能发生）。
 */
export const APP_WRITABLE_FIELDS = [
  'clientId',
  'displayName',
  'description',
  'category',
  'iconUrl',
  'launchUrl',
  'sortOrder',
] as const;

/** 可写字段名。 */
export type AppWritableField = (typeof APP_WRITABLE_FIELDS)[number];

/**
 * 平台错误码白名单（**恰好 6 个**，`acceptance.md` §全局约定）。
 *
 * 码逐字等于棒 1 的 `crates/platform-app/src/error.rs` —— 改一个要改两处。
 * ⚠️ 重试 6 个之外**没有**第 7 个（`APP_REGISTRY_MISMATCH` 是启动对账失败，
 * 不进 HTTP 响应；`APP_INVALID_SCOPE_PROJECTION` 构造不出来）。
 *
 * ⚠️ 本文件**只管码**，`error.APP_*` 的**文案**归 REQ-016 棒 1
 * （`packages/i18n/src/locales/*.json` 的错误域）。
 */
export const APP_ERROR_CODES = [
  'APP_NOT_FOUND',
  'APP_DUPLICATED',
  'APP_CLIENT_INVALID',
  'APP_LAUNCH_URL_INVALID',
  'APP_ICON_URL_INVALID',
  'APP_INVALID_REQUEST',
] as const;

/** 平台错误码。 */
export type AppErrorCode = (typeof APP_ERROR_CODES)[number];

/**
 * `launch_url` / `icon_url` 允许的 scheme。
 *
 * 与后端默认（`app.icon.allowHttp = false`）**不**逐字对齐是**有意**的：
 * 前端是**渲染防线**（挡住 `javascript:` → XSS），不是策略的权威判定；
 * `http:` 是否允许由租户配置决定，前端不发这个判定，只挡「绝不是 URL」的 scheme。
 * 真正的权威拒绝在后端（`APP_LAUNCH_URL_INVALID` / `APP_ICON_URL_INVALID`）。
 */
export const APP_ALLOWED_URL_SCHEMES = ['https:', 'http:'] as const;

/** 空值的展示文案（没有描述 / 没有入口时）。 */
export const APP_EMPTY_VALUE = '-';

/* -------------------------------------------------------------------------- */
/* 类型                                                                        */
/* -------------------------------------------------------------------------- */

/** 一个应用（响应侧字段；camelCase —— 契约 C13 / D258）。 */
export interface AppView {
  /** 应用 id（后端生成）。 */
  readonly appId: string;
  /** 所属的 OAuth client（**引用**，不复制 —— D262）。 */
  readonly clientId: string;
  /** 展示名。 */
  readonly displayName: string;
  /** 描述（可空）。 */
  readonly description: string | null;
  /** 分类（工作台按它分组）。 */
  readonly category: string;
  /** 图标地址（可空）。 */
  readonly iconUrl: string | null;
  /** 应用入口地址（可空 —— 可空时工作台仍显示、导航不下发，D279）。 */
  readonly launchUrl: string | null;
  /** 排序值（组内升序，并列时按 `displayName`）。 */
  readonly sortOrder: number;
  /** 状态。 */
  readonly status: string;
  /** 权限范围（client `allowed_scopes` 的**只读投影**，D266）。 */
  readonly scopes: readonly string[];
  /** 接入时间（UTC、RFC3339、微秒 6 位）。 */
  readonly createdAt: string;
  /** 接入人。 */
  readonly createdBy: string;
}

/** 工作台的一个分类组。 */
export interface AppGroup {
  /** 分类名（组按它字典序升序）。 */
  readonly category: string;
  /** 组内的应用（组内按 `sortOrder`、并列按 `displayName`）。 */
  readonly apps: readonly AppView[];
}

/** 工作台响应。 */
export interface AppWorkspace {
  /** 分类组。 */
  readonly groups: readonly AppGroup[];
}

/** 导航项 —— `module` 来源（来自 manifest 的 `menus[]`）。 */
export interface ModuleNavigationItem {
  /** 来源。 */
  readonly source: 'module';
  /** 菜单文案（**已渲染的中文文案**，不是翻译键 —— D284）。 */
  readonly label: string;
  /** 图标（可空）。 */
  readonly icon: string | null;
  /** 站内路径（以 `/` 开头）。 */
  readonly target: string;
  /** 跳转语义（`module` 恒为 `internal`）。 */
  readonly kind: 'internal';
  /** 所需权限点（`null` = 登录即可见）。 */
  readonly requiredPermission: string | null;
}

/** 导航项 —— `app` 来源（来自 `platform_app`）。 */
export interface AppNavigationItem {
  /** 来源。 */
  readonly source: 'app';
  /** 应用 id。 */
  readonly appId: string;
  /** 应用展示名（`platform_app.display_name`）。 */
  readonly label: string;
  /** 图标（`platform_app.icon_url`）。 */
  readonly icon: string | null;
  /** 外部 URL（`platform_app.launch_url`）。 */
  readonly target: string;
  /** 跳转语义（`app` 恒为 `external`）。 */
  readonly kind: 'external';
  /** 应用本身不是权限点 → 恒为 `null`。 */
  readonly requiredPermission: null;
}

/** 导航项（判别联合 —— D284 / S-09：字段按来源**分别**定义）。 */
export type NavigationItem = ModuleNavigationItem | AppNavigationItem;

/** 建应用 / 改应用的输入（全部可选 —— `PATCH` 是任意子集）。 */
export interface AppInput {
  /** 所属 client（**建的时候必填**）。 */
  readonly clientId?: string;
  /** 展示名（建的时候必填）。 */
  readonly displayName?: string;
  /** 描述。 */
  readonly description?: string;
  /** 分类。 */
  readonly category?: string;
  /** 图标地址。 */
  readonly iconUrl?: string;
  /** 入口地址。 */
  readonly launchUrl?: string;
  /** 排序值。 */
  readonly sortOrder?: number;
}

/** 记一次访问的响应。 */
export interface AppAccessResult {
  /** 后端状态（恒为 `recorded`）。 */
  readonly status: string;
  /** 累计访问次数（**UPSERT 累加**后的值）。 */
  readonly accessCount: number;
}

/* -------------------------------------------------------------------------- */
/* 规整（形状不对 → 安全默认值，**不抛错**）                                    */
/* -------------------------------------------------------------------------- */

/** 值是不是一个可用的对象（`null` / 数组不算）。 */
function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

/** 取文本（缺键 → 空串）。 */
function text(value: unknown): string {
  if (typeof value === 'string') {
    return value;
  }
  if (value === undefined || value === null) {
    return '';
  }
  return String(value);
}

/** 取可空文本（缺键 / 空串 → `null`）。 */
function optionalText(value: unknown): string | null {
  const rendered = text(value);
  return rendered === '' ? null : rendered;
}

/** 取整数（非有限数 → `fallback`）。 */
function integer(value: unknown, fallback: number): number {
  if (typeof value === 'number' && Number.isFinite(value)) {
    return Math.trunc(value);
  }
  if (typeof value === 'string' && value.trim() !== '' && Number.isFinite(Number(value))) {
    return Math.trunc(Number(value));
  }
  return fallback;
}

/** 取字符串数组（非数组 → 空数组；元素逐个转文本）。 */
function textList(value: unknown): string[] {
  if (!Array.isArray(value)) {
    return [];
  }
  return value.map((entry) => text(entry));
}

/** 把一个原始对象规整成 `AppView`（字段缺失 → 安全默认值）。 */
export function toAppView(raw: unknown): AppView {
  const record = isRecord(raw) ? raw : {};
  return {
    appId: text(record['appId']),
    clientId: text(record['clientId']),
    displayName: text(record['displayName']),
    description: optionalText(record['description']),
    category: text(record['category']),
    iconUrl: optionalText(record['iconUrl']),
    launchUrl: optionalText(record['launchUrl']),
    sortOrder: integer(record['sortOrder'], 0),
    status: text(record['status']),
    scopes: textList(record['scopes']),
    createdAt: text(record['createdAt']),
    createdBy: text(record['createdBy']),
  };
}

/**
 * 解析**数组**型响应（`GET /api/apps` / `recent` / `frequent`）。
 *
 * ⚠️ 这三个端点返回的是**裸数组**（不是 `{ items: [] }`）→ 包了对象的形状
 * 一律当「没有数据」处理，**不要**顺手兼容（那会把形状错误掩盖成空列表之外的东西）。
 */
export function parseAppList(raw: unknown): AppView[] {
  if (!Array.isArray(raw)) {
    return [];
  }
  return raw.map((entry) => toAppView(entry));
}

/** 解析工作台响应（`{ groups: [...] }`）。 */
export function parseWorkspace(raw: unknown): AppWorkspace {
  const record = isRecord(raw) ? raw : {};
  const rawGroups = Array.isArray(record['groups']) ? record['groups'] : [];
  const groups: AppGroup[] = [];
  for (const entry of rawGroups) {
    if (!isRecord(entry)) {
      continue;
    }
    groups.push({
      category: text(entry['category']),
      apps: parseAppList(entry['apps']),
    });
  }
  return { groups };
}

/**
 * 规整一个导航项。
 *
 * `kind` **由来源推出**，不采信响应里的 `kind`：`module` 恒 `internal`、
 * `app` 恒 `external`（D284）。这样即使后端把 `kind` 发错，
 * 外部应用也**不会**被当成站内路径塞进 SPA 路由表（`routesFromNavigation` 的输入
 * 因此与「来源」一致，静默失败无从发生）。
 *
 * 形状不可用（来源不认识 / `target` 为空 / `app` 项缺 `appId`）→ `undefined`
 * （跳过这一项，而不是造一条点不动的空项）。
 */
export function toNavigationItem(raw: unknown): NavigationItem | undefined {
  if (!isRecord(raw)) {
    return undefined;
  }
  const target = text(raw['target']);
  if (target === '') {
    return undefined;
  }
  const label = text(raw['label']);
  const icon = optionalText(raw['icon']);
  const source = raw['source'];
  if (source === 'module') {
    return {
      source: 'module',
      label,
      icon,
      target,
      kind: 'internal',
      requiredPermission: optionalText(raw['requiredPermission']),
    };
  }
  if (source === 'app') {
    const appId = text(raw['appId']);
    if (appId === '') {
      return undefined;
    }
    return {
      source: 'app',
      appId,
      label,
      icon,
      target,
      kind: 'external',
      requiredPermission: null,
    };
  }
  return undefined;
}

/** 解析导航响应（`{ items: [...] }`）。 */
export function parseNavigation(raw: unknown): NavigationItem[] {
  const record = isRecord(raw) ? raw : {};
  const rawItems = Array.isArray(record['items']) ? record['items'] : [];
  const items: NavigationItem[] = [];
  for (const entry of rawItems) {
    const item = toNavigationItem(entry);
    if (item !== undefined) {
      items.push(item);
    }
  }
  return items;
}

/** 解析「记一次访问」的响应（形状不对 → 0 次）。 */
export function parseAccessResult(raw: unknown): AppAccessResult {
  const record = isRecord(raw) ? raw : {};
  return {
    status: text(record['status']),
    accessCount: integer(record['accessCount'], 0),
  };
}

/* -------------------------------------------------------------------------- */
/* 确定性排序（D267 —— 组按 category、组内按 sortOrder 再 displayName）          */
/* -------------------------------------------------------------------------- */

/**
 * 按码点比较字符串（**与后端的 `str` 比较同序**）。
 *
 * ⚠️ 刻意**不用** `localeCompare`：它随用户语言变化，会给出与后端不同的顺序
 * —— 前端若「按语言重排」，同一个响应在不同语言下顺序不同，
 * 与 AC-S-160 的「确定性排序」直接冲突。
 */
function compareText(left: string, right: string): number {
  if (left === right) {
    return 0;
  }
  return left < right ? -1 : 1;
}

/** 组内排序：`sortOrder` 升序；并列时 `displayName` 升序（D267）。 */
export function sortApps(apps: readonly AppView[]): AppView[] {
  return [...apps].sort((left, right) => {
    if (left.sortOrder !== right.sortOrder) {
      return left.sortOrder - right.sortOrder;
    }
    return compareText(left.displayName, right.displayName);
  });
}

/**
 * 工作台排序：组按 `category` 升序、组内按 `sortApps`。
 *
 * ⚠️ **后端已经是权威**（AC-S-160 的判据是 `GET /api/apps/workspace` 的响应体顺序）。
 * 前端这里再排一次是**展示层的确定性保障**，代价是它会**掩盖**后端的错序
 * （后端若返回乱序，前端照样排好 → E2E 看不见）。
 * → AC-S-160 的真判据**必须**由棒 1 的集成测试（响应体逐字节比较）承担，
 * 本函数**不能**替代它（见 `feature-meta/req-016-dev-frontend.md` 的说明）。
 */
export function sortWorkspace(groups: readonly AppGroup[]): AppGroup[] {
  return [...groups]
    .sort((left, right) => compareText(left.category, right.category))
    .map((group) => ({ category: group.category, apps: sortApps(group.apps) }));
}

/* -------------------------------------------------------------------------- */
/* 入口地址的安全渲染（`javascript:` / `data:` 必须挡住）                        */
/* -------------------------------------------------------------------------- */

/**
 * 判定一个入口地址能不能放进 `href` / 用来整页跳转。
 *
 * 只放行 `http:` / `https:`；`javascript:`（脚本注入）、`data:`（可嵌 HTML →
 * 打开即 XSS）、`ftp:` 以及解析失败的串一律 `undefined`。
 *
 * 判定用 **URL 解析器**（比正则准）：`new URL(value)` 的 `protocol` 是规范化后的值，
 * `JavaScript:` / ` java\nscript:` 之类的大小写 / 空白变体会被统一成 `javascript:`。
 */
export function safeLaunchUrl(value: string | null | undefined): string | undefined {
  if (value === null || value === undefined) {
    return undefined;
  }
  const trimmed = value.trim();
  if (trimmed === '') {
    return undefined;
  }
  let parsed: URL;
  try {
    parsed = new URL(trimmed);
  } catch {
    return undefined;
  }
  return (APP_ALLOWED_URL_SCHEMES as readonly string[]).includes(parsed.protocol)
    ? trimmed
    : undefined;
}

/** 「打开应用」的决策（**纯函数** —— 点击链由 E2E 兜底，决策在这里单测）。 */
export type AppOpenAction =
  | { readonly kind: 'link'; readonly url: string }
  | { readonly kind: 'none' };

/**
 * 决定一个应用能不能打开、打开到哪。
 *
 * `launch_url` 为空 / scheme 非法 → `{ kind: 'none' }` → **不渲染可点入口**
 * （工作台仍显示这张卡片，只是没有入口 —— D279：工作台不按 `launch_url` 过滤）。
 * 这是 AC-S-162 步骤 6（置空入口后不下发）在**前端侧**的对应判据。
 */
export function appOpenAction(app: AppView): AppOpenAction {
  const url = safeLaunchUrl(app.launchUrl);
  return url === undefined ? { kind: 'none' } : { kind: 'link', url };
}

/** 浏览器环境（可注入 → 单测不真的导航）。 */
export interface AppPageEnv {
  /** 整页跳转（`location.href = url` —— **不**经过 SPA 路由）。 */
  assign(url: string): void;
}

/**
 * 一个导航项被点击时该怎么处置（**纯函数** —— 决定「整页跳」还是「SPA 内跳」）。
 *
 * # 为什么必须是纯函数
 *
 * `svelte/server` 的 `render` 不能点击，而这条判据的核心恰恰是「点击之后
 * 到底走哪条跳转路径」：
 *
 * | 来源 | 处置 | 为什么 |
 * |---|---|---|
 * | `module`（`kind="internal"`） | 站内链接（交给 SPA 路由） | `target` 是本 SPA 的路径 |
 * | `app`（`kind="external"`） | **整页跳转**（`location.assign`） | 第三方应用是独立站点，**没有本 SPA 的组件**（D278） |
 *
 * 把两类混成一种，外部 URL 会被当成站内路径 → SPA 路由 `match()` 不到
 * → **静默失败**（AC-S-163 步骤 2 验的就是这件事）。
 *
 * `target` 为空的项在规整层已被跳过 → 这里只需按 `kind` 分派。
 */
export type NavigationClickTarget =
  | { readonly kind: 'external'; readonly url: string }
  | { readonly kind: 'internal'; readonly path: string };

/** 判定一个导航项点击后的跳转形态（见 `NavigationClickTarget` 的说明）。 */
export function navigationClickTarget(item: NavigationItem): NavigationClickTarget {
  if (item.kind === 'external') {
    return { kind: 'external', url: item.target };
  }
  return { kind: 'internal', path: item.target };
}

/** 取全局 `window`（SSR / Node 单测里没有）。 */
function globalWindow(): (Window & typeof globalThis) | undefined {
  return (globalThis as { window?: Window & typeof globalThis }).window;
}

/** 浏览器环境的默认实现（非浏览器环境 → 跳转为**无操作**，页面不崩）。 */
export function browserAppEnv(): AppPageEnv {
  return {
    assign(url: string): void {
      const win = globalWindow();
      if (win === undefined) {
        return;
      }
      // `location.assign` 是**整页导航**：不触发 `history.pushState`
      // （AC-S-163 步骤 3 的判据：pushState 调用次数 == 0）。
      win.location.assign(url);
    },
  };
}

/* -------------------------------------------------------------------------- */
/* 请求体构造（只发白名单字段）                                                 */
/* -------------------------------------------------------------------------- */

/**
 * 造 `POST /api/apps` 的请求体。
 *
 * 只发**白名单 + 有值**的字段：空串与 `undefined` 一律不发
 * （`launchUrl: ""` 会被后端判成非法 scheme，而用户的意思是「没填」）。
 * `sortOrder` 是数字 → `0` 是**合法值**，不能当空值丢掉。
 */
export function buildCreateBody(input: AppInput): Record<string, unknown> {
  const body: Record<string, unknown> = {};
  for (const field of APP_WRITABLE_FIELDS) {
    if (field === 'sortOrder') {
      if (input.sortOrder !== undefined && Number.isFinite(input.sortOrder)) {
        body['sortOrder'] = Math.trunc(input.sortOrder);
      }
      continue;
    }
    const value = input[field];
    if (typeof value === 'string' && value !== '') {
      body[field] = value;
    }
  }
  return body;
}

/**
 * 造 `PATCH /api/apps/{appId}` 的请求体。
 *
 * 与 `buildCreateBody` **同一个白名单**，区别只在语义：
 * `PATCH` 是「任意子集」→ 只发调用方显式给了值的键（`undefined` 不发）。
 * ⚠️ **置空 `launch_url` 不走这里** —— `null` 与「省略键」在 serde 里等价（D283）
 * → 必须用 `DELETE /api/apps/{appId}/launch-url`。
 */
export function buildPatchBody(input: AppInput): Record<string, unknown> {
  return buildCreateBody(input);
}

/** 表格行（展示用的扁平结构）。 */
export interface AppRow {
  /** 应用 id（行键）。 */
  readonly appId: string;
  /** 展示名。 */
  readonly displayName: string;
  /** 所属 client。 */
  readonly clientId: string;
  /** 分类。 */
  readonly category: string;
  /** 状态（未知值**逐字**显示）。 */
  readonly status: string;
  /** 排序值（展示用文本）。 */
  readonly sortOrder: string;
  /** 入口地址（没有 → `APP_EMPTY_VALUE`）。 */
  readonly launchUrl: string;
}

/** 把应用列表转成展示行。 */
export function toAppRows(apps: readonly AppView[]): AppRow[] {
  return apps.map((app) => ({
    appId: app.appId,
    displayName: app.displayName,
    clientId: app.clientId,
    category: app.category,
    status: app.status,
    sortOrder: String(app.sortOrder),
    launchUrl: app.launchUrl ?? APP_EMPTY_VALUE,
  }));
}

/** 状态文案的翻译键（未知值 → `undefined`，调用方逐字显示原文）。 */
export function appStatusKey(status: string): string | undefined {
  if (status === 'active') {
    return 'app.registryStatusActive';
  }
  if (status === 'hidden') {
    return 'app.registryStatusHidden';
  }
  return undefined;
}

/* -------------------------------------------------------------------------- */
/* 接口调用                                                                    */
/* -------------------------------------------------------------------------- */

/** 列出全部应用（管理视角，需要 `app:registry:read`）。 */
export async function listApps(client: ApiClient): Promise<AppView[]> {
  const raw = await client.request<unknown>({ path: APPS_PATH });
  return parseAppList(raw);
}

/** 读工作台（登录即可）。 */
export async function loadWorkspace(client: ApiClient): Promise<AppWorkspace> {
  const raw = await client.request<unknown>({ path: APPS_WORKSPACE_PATH });
  return parseWorkspace(raw);
}

/** 读最近访问（登录即可）。 */
export async function loadRecentApps(client: ApiClient): Promise<AppView[]> {
  const raw = await client.request<unknown>({ path: APPS_RECENT_PATH });
  return parseAppList(raw);
}

/** 读常用应用（登录即可）。 */
export async function loadFrequentApps(client: ApiClient): Promise<AppView[]> {
  const raw = await client.request<unknown>({ path: APPS_FREQUENT_PATH });
  return parseAppList(raw);
}

/** 读导航项（登录即可；按权限点过滤由后端做）。 */
export async function loadNavigation(client: ApiClient): Promise<NavigationItem[]> {
  const raw = await client.request<unknown>({ path: APPS_NAVIGATION_PATH });
  return parseNavigation(raw);
}

/** 记一次访问（UPSERT 累加；失败不挡跳转，由调用方决定是否忽略）。 */
export async function recordAppAccess(
  client: ApiClient,
  appId: string,
): Promise<AppAccessResult> {
  const raw = await client.request<unknown>({
    path: appAccessPath(appId),
    method: 'POST',
  });
  return parseAccessResult(raw);
}

/** 注册应用（201）。 */
export async function createApp(client: ApiClient, input: AppInput): Promise<AppView> {
  const raw = await client.request<unknown>({
    path: APPS_PATH,
    method: 'POST',
    body: buildCreateBody(input),
  });
  return toAppView(raw);
}

/** 修改应用（200，返回改后的整条）。 */
export async function updateApp(
  client: ApiClient,
  appId: string,
  input: AppInput,
): Promise<AppView> {
  const raw = await client.request<unknown>({
    path: appPath(appId),
    method: 'PATCH',
    body: buildPatchBody(input),
  });
  return toAppView(raw);
}

/** 删除应用（不动它引用的 client —— D262 / D264：cascade 是 client → app 方向）。 */
export async function deleteApp(client: ApiClient, appId: string): Promise<void> {
  await client.request<unknown>({ path: appPath(appId), method: 'DELETE' });
}

/** 把 `launch_url` 置 NULL（**专用端点**，204 —— D283）。 */
export async function clearAppLaunchUrl(client: ApiClient, appId: string): Promise<void> {
  await client.request<unknown>({ path: appLaunchUrlPath(appId), method: 'DELETE' });
}
