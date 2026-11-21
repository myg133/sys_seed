/**
 * 通知系统接口封装（REQ-015 / AC-S-167 · 168 · 169 · 172 · 173 —— 前端侧）。
 *
 * # 字段名 = camelCase（契约 C13 / D258）
 *
 * `/api/notify*` 的 JSON 字段**一律 camelCase**：`templateId` / `templateCode` /
 * `titleTemplate` / `bodyTemplate` / `deliveryId` / `attemptCount` / `nextAttemptAt` /
 * `lastError` / `providerMessageId` / `renderedTitle` / `renderedBody` / `deliveredAt`。
 * 逐字对照 `design-summary.md` §3.1 的字段表 —— 改一个要改两处。
 *
 * # 三类响应形状，不要混着用
 *
 * | 端点 | 形状 |
 * |---|---|
 * | `GET /api/notify/templates` | **数组** `[TemplateView]` |
 * | `GET /api/notify/deliveries` | **数组** `[DeliveryView]` |
 * | `GET /api/notify/preferences` | **数组** `[PreferenceView]` |
 * | `POST /api/notify/templates/{id}/preview` | `{ title, body }` |
 *
 * 把数组当 `{ items: [] }` 读会**静默**得到「0 条」而不是报错 ——
 * 所以每个解析函数都独立、且形状不对时**不抛错**（列表页不白屏）。
 *
 * # ⚠️ 模板只支持 `{{variable}}`（D293）
 *
 * **不做**条件 / 循环 / 表达式 —— 编辑界面的变量提示也只认
 * `^[a-z][a-z0-9_]*$` 的白名单变量名。前端**不得**提供「插入 `{{#if}}`」这类按钮：
 * 后端会以 `NOTIFY_TEMPLATE_INVALID_PLACEHOLDER` 拒绝，前端先给出就是误导。
 *
 * # ⚠️ `system` 类不可退订（D295）
 *
 * 偏好界面上 `system` 必须**没有**开关（不是「开关但点了报错」）。
 */

import type { ApiClient } from '@base/sdk';

/* -------------------------------------------------------------------------- */
/* 路径常量（逐字 —— 与后端 `src/http.rs` 对照用）                              */
/* -------------------------------------------------------------------------- */

/** 模板列表 / 新建。 */
export const NOTIFY_TEMPLATES_PATH = '/api/notify/templates';

/** 投递记录。 */
export const NOTIFY_DELIVERIES_PATH = '/api/notify/deliveries';

/** 订阅偏好。 */
export const NOTIFY_PREFERENCES_PATH = '/api/notify/preferences';

/** 单个模板路径。 */
export function notifyTemplatePath(templateId: string): string {
  return `${NOTIFY_TEMPLATES_PATH}/${encodeURIComponent(templateId)}`;
}

/** 预览路径。 */
export function notifyTemplatePreviewPath(templateId: string): string {
  return `${notifyTemplatePath(templateId)}/preview`;
}

/** 重投死信路径。 */
export function notifyResendPath(deliveryId: string): string {
  return `${NOTIFY_DELIVERIES_PATH}/${encodeURIComponent(deliveryId)}/resend`;
}

/* -------------------------------------------------------------------------- */
/* 权限点（契约 C4：`module:resource:action`）                                   */
/* -------------------------------------------------------------------------- */

/** 查看模板（模板页的路由守卫用它）。 */
export const NOTIFY_TEMPLATE_READ_PERMISSION = 'notify:template:read';

/** 维护模板。 */
export const NOTIFY_TEMPLATE_WRITE_PERMISSION = 'notify:template:write';

/** 查看投递记录 + 重投死信。 */
export const NOTIFY_DELIVERY_READ_PERMISSION = 'notify:delivery:read';

/* -------------------------------------------------------------------------- */
/* 白名单                                                                      */
/* -------------------------------------------------------------------------- */

/** 通道名（**恰好 7 个**：3 个真实现 + 4 个未配置，D292）。 */
export const NOTIFY_CHANNELS = [
  'in_app',
  'smtp',
  'webhook',
  'sms',
  'dingtalk',
  'wecom',
  'feishu',
] as const;

/** 通道名。 */
export type NotifyChannel = (typeof NOTIFY_CHANNELS)[number];

/**
 * **有真实现**的通道（3 个）。
 *
 * 编辑界面对其余 4 个要**显式标注「未配置」** —— 让管理员知道选了也发不出去，
 * 而不是配了模板才发现投递全进死信。
 */
export const NOTIFY_IMPLEMENTED_CHANNELS = ['in_app', 'smtp', 'webhook'] as const;

/** 通知分类。 */
export const NOTIFY_CATEGORIES = ['system', 'marketing', 'transactional'] as const;

/** 通知分类。 */
export type NotifyCategory = (typeof NOTIFY_CATEGORIES)[number];

/**
 * **可退订**的分类（D295：`system` 不可退订）。
 *
 * ⚠️ 偏好界面用它决定「有没有开关」——
 * `system` 必须**没有**开关，不是「开关但点了 400」。
 */
export const NOTIFY_OPTIONAL_CATEGORIES = ['marketing', 'transactional'] as const;

/** 投递状态。 */
export const NOTIFY_DELIVERY_STATUSES = [
  'pending',
  'delivering',
  'delivered',
  'dead_letter',
] as const;

/** 投递状态。 */
export type NotifyDeliveryStatus = (typeof NOTIFY_DELIVERY_STATUSES)[number];

/**
 * 平台错误码白名单（**恰好 8 个**，`acceptance.md` §全局约定 / D303）。
 *
 * 码逐字等于后端 `crates/platform-notify/src/error.rs` 的 `ALL_CODES` —— 改一个要改两处。
 * ⚠️ 4 个 `notify.*` 审计**事件码不在这里**（D223：事件码不是 HTTP 错误码）。
 */
export const NOTIFY_ERROR_CODES = [
  'NOTIFY_TEMPLATE_NOT_FOUND',
  'NOTIFY_TEMPLATE_DUPLICATED',
  'NOTIFY_TEMPLATE_MISSING_VARIABLE',
  'NOTIFY_TEMPLATE_INVALID_PLACEHOLDER',
  'NOTIFY_CHANNEL_NOT_CONFIGURED',
  'NOTIFY_CATEGORY_MANDATORY',
  'NOTIFY_NOT_DEAD_LETTER',
  'NOTIFY_SENSITIVE_PAYLOAD',
] as const;

/** 平台错误码。 */
export type NotifyErrorCode = (typeof NOTIFY_ERROR_CODES)[number];

/** 错误码 → i18n 键（`error.<CODE>`，与 `packages/i18n` 的扁平键一致）。 */
export function notifyErrorKey(code: string): string {
  return `error.${code}`;
}

/** 该错误码是否已登记（用于「未知码也要显示得出来」的兜底）。 */
export function isKnownNotifyErrorCode(code: string): code is NotifyErrorCode {
  return (NOTIFY_ERROR_CODES as readonly string[]).includes(code);
}

/* -------------------------------------------------------------------------- */
/* 视图类型                                                                    */
/* -------------------------------------------------------------------------- */

/** 模板（camelCase）。 */
export interface TemplateView {
  templateId: string;
  templateCode: string;
  channel: string;
  locale: string;
  category: string;
  titleTemplate: string;
  bodyTemplate: string;
  createdAt: string;
  updatedAt: string;
}

/** 投递记录（camelCase）。 */
export interface DeliveryView {
  deliveryId: string;
  templateCode: string;
  channel: string;
  category: string;
  recipient: string;
  locale: string;
  renderedTitle: string;
  renderedBody: string;
  status: string;
  attemptCount: number;
  nextAttemptAt: string;
  lastError: string | null;
  providerMessageId: string | null;
  createdAt: string;
  updatedAt: string;
  deliveredAt: string | null;
}

/** 订阅偏好（camelCase）。 */
export interface PreferenceView {
  category: string;
  optedOut: boolean;
  updatedAt: string;
}

/** 预览结果。 */
export interface PreviewResult {
  title: string;
  body: string;
}

/** 建 / 改模板的请求体。 */
export interface TemplateUpsertBody {
  templateCode: string;
  channel: string;
  locale: string;
  category: string;
  titleTemplate: string;
  bodyTemplate: string;
}

/** 设偏好的请求体。 */
export interface SetPreferenceBody {
  category: string;
  optedOut: boolean;
}

/* -------------------------------------------------------------------------- */
/* 解析（形状不对时返回空，不抛错）                                             */
/* -------------------------------------------------------------------------- */

function asArray<T>(value: unknown): T[] {
  return Array.isArray(value) ? (value as T[]) : [];
}

/** 解析模板列表。 */
export function parseTemplates(payload: unknown): TemplateView[] {
  return asArray<TemplateView>(payload);
}

/** 解析投递记录列表。 */
export function parseDeliveries(payload: unknown): DeliveryView[] {
  return asArray<DeliveryView>(payload);
}

/** 解析偏好列表。 */
export function parsePreferences(payload: unknown): PreferenceView[] {
  return asArray<PreferenceView>(payload);
}

/** 解析预览结果（形状不对 → `null`）。 */
export function parsePreview(payload: unknown): PreviewResult | null {
  if (typeof payload !== 'object' || payload === null) {
    return null;
  }
  const record = payload as Record<string, unknown>;
  const title = record['title'];
  const body = record['body'];
  if (typeof title !== 'string' || typeof body !== 'string') {
    return null;
  }
  return { title, body };
}

/* -------------------------------------------------------------------------- */
/* 客户端                                                                      */
/* -------------------------------------------------------------------------- */

/** 通知系统的接口客户端。 */
export class NotifyApi {
  constructor(private readonly client: ApiClient) {}

  /** 列出模板。 */
  async listTemplates(): Promise<TemplateView[]> {
    return parseTemplates(await this.client.get(NOTIFY_TEMPLATES_PATH));
  }

  /** 读单个模板。 */
  async getTemplate(templateId: string): Promise<TemplateView | null> {
    const payload = await this.client.get(notifyTemplatePath(templateId));
    if (typeof payload !== 'object' || payload === null) {
      return null;
    }
    return payload as TemplateView;
  }

  /** 建模板（201）。 */
  async createTemplate(body: TemplateUpsertBody): Promise<TemplateView> {
    return (await this.client.post(NOTIFY_TEMPLATES_PATH, body)) as TemplateView;
  }

  /**
   * 改模板。
   *
   * ⚠️ `ApiClient` **没有 `patch` 快捷方式**（`packages/sdk/src/client.ts:119-135`
   * 只有 `get` / `post` / `put` / `delete` / `request`）→ 走 `request` 显式指定方法。
   */
  async updateTemplate(
    templateId: string,
    body: Partial<TemplateUpsertBody>,
  ): Promise<TemplateView> {
    return (await this.client.request<TemplateView>({
      method: 'PATCH',
      path: notifyTemplatePath(templateId),
      body,
    })) as TemplateView;
  }

  /** 删模板（204）。 */
  async deleteTemplate(templateId: string): Promise<void> {
    await this.client.delete(notifyTemplatePath(templateId));
  }

  /** 预览（**不落库**）。 */
  async previewTemplate(
    templateId: string,
    payload: Record<string, string>,
  ): Promise<PreviewResult | null> {
    const raw = await this.client.post(notifyTemplatePreviewPath(templateId), { payload });
    return parsePreview(raw);
  }

  /** 列出投递记录。 */
  async listDeliveries(): Promise<DeliveryView[]> {
    return parseDeliveries(await this.client.get(NOTIFY_DELIVERIES_PATH));
  }

  /** 重投死信（200；非死信 → 409 `NOTIFY_NOT_DEAD_LETTER`）。 */
  async resendDelivery(deliveryId: string): Promise<DeliveryView> {
    return (await this.client.post(notifyResendPath(deliveryId), {})) as DeliveryView;
  }

  /** 读偏好。 */
  async listPreferences(): Promise<PreferenceView[]> {
    return parsePreferences(await this.client.get(NOTIFY_PREFERENCES_PATH));
  }

  /** 设偏好。 */
  async setPreference(body: SetPreferenceBody): Promise<PreferenceView> {
    return (await this.client.put(NOTIFY_PREFERENCES_PATH, body)) as PreferenceView;
  }
}

/* -------------------------------------------------------------------------- */
/* 纯函数（可单测，不依赖网络）                                                 */
/* -------------------------------------------------------------------------- */

/**
 * 该分类是否可退订（D295）。
 *
 * 偏好界面据此决定「有没有开关」：`system` → **没有**开关。
 */
export function isCategoryOptional(category: string): boolean {
  return (NOTIFY_OPTIONAL_CATEGORIES as readonly string[]).includes(category);
}

/**
 * 该通道是否有真实现（D292）。
 *
 * 编辑界面对未配置的通道要**显式标注**「未配置」——
 * 让管理员知道选了也发不出去，而不是配了模板才发现投递全进死信。
 */
export function isChannelImplemented(channel: string): boolean {
  return (NOTIFY_IMPLEMENTED_CHANNELS as readonly string[]).includes(channel);
}

/**
 * 该投递能否重投（只有死信可以，D298）。
 *
 * ⚠️ 这只是「按钮该不该亮」的前端判断 —— **真实判定在后端的影响行数守卫**。
 * 前端先判一次是为了少给用户一个必然 409 的按钮，不是替代后端。
 */
export function isResendable(status: string): boolean {
  return status === 'dead_letter';
}

/**
 * 从模板正文里抽出 `{{variable}}` 变量名（去重、保持出现顺序）。
 *
 * 与后端 `template::extract_variables` 的**白名单一致**：`^[a-z][a-z0-9_]*$`。
 * 非法占位符**不**返回（后端会拒绝保存；这里只是给编辑界面做提示）。
 */
export function extractTemplateVariables(template: string): string[] {
  const seen = new Set<string>();
  const result: string[] = [];
  const pattern = /\{\{([^{}]*)\}\}/g;
  let match: RegExpExecArray | null;
  while ((match = pattern.exec(template)) !== null) {
    const name = match[1] ?? '';
    if (!/^[a-z][a-z0-9_]*$/.test(name)) {
      continue;
    }
    if (!seen.has(name)) {
      seen.add(name);
      result.push(name);
    }
  }
  return result;
}

/**
 * 模板是否含**非法**占位符（后端会以 `NOTIFY_TEMPLATE_INVALID_PLACEHOLDER` 拒绝）。
 *
 * 编辑界面用它做保存前的提示 —— 但**不阻止**保存（后端的判定才是权威）。
 */
export function hasInvalidPlaceholders(template: string): boolean {
  const pattern = /\{\{([^{}]*)\}\}/g;
  let match: RegExpExecArray | null;
  while ((match = pattern.exec(template)) !== null) {
    const name = match[1] ?? '';
    if (!/^[a-z][a-z0-9_]*$/.test(name)) {
      return true;
    }
  }
  return false;
}
