/**
 * 壳层的 i18n 运行时（REQ-021 / AC-S-094 · 095 · 099）。
 *
 * # 用户偏好只在前端（D112）
 *
 * 用户表**没有** locale / timezone / currency 列，所以用户级偏好存 `sessionStorage`
 * （与 REQ-011 的 D99 同一取舍：单标签页 + 单次会话）。要「跨设备同步」需另立需求。
 *
 * # 首次进入的取值顺序（design §7.2）
 *
 * ```
 * sessionStorage 用户偏好 → navigator.language → 租户配置（GET /api/i18n/context）→ 平台默认
 * ```
 *
 * 时区**不参与请求头协商**（§3.4）：`sessionStorage → 租户配置 → 平台默认`。
 * 这里也**不**把浏览器时区自动写成用户偏好 —— 那会让租户配置永远无法生效
 * （AC-S-095 的「租户生效」就验不出来了）。
 *
 * # 把用户偏好带上来（D116）
 *
 * 后端接受 `?locale=` / `?timezone=` 作为「用户偏好」这一级（不持久化、不跨请求记忆）。
 * 所以 `refreshContext()` 会在请求里带上 `sessionStorage` 里的值 —— 否则
 * `source === "user"` 永远不可能出现（AC-S-090 步骤 8）。
 *
 * # 切换语言后已渲染组件立即更新（AC-S-099 步骤 5）
 *
 * 运行时实现了 Svelte 的 store 契约（`subscribe(run)` 立即回放当前值），
 * 组件用 `$i18n` 订阅；`setLocale` 不刷新页面。
 */

import {
  BASE_LOCALE,
  LOCALE_OPTIONS,
  createI18n,
  isRegisteredLocale,
  registeredLocales,
  type I18n,
  type LocaleOption,
} from '@base/i18n';

/** 用户偏好的存储键（带版本号：结构变化时旧结构不会被当成新结构读）。 */
export const I18N_PREFERENCE_KEY = 'base.shell.i18n.v1';

/** 协商 / 上下文的观测通道（D110）。 */
export const I18N_CONTEXT_PATH = '/api/i18n/context';

/** 平台默认值（与 manifest 的 `default` 一致；接口不可用时的兜底）。 */
export const PLATFORM_DEFAULT_LOCALE = BASE_LOCALE;
export const PLATFORM_DEFAULT_TIMEZONE = 'Asia/Shanghai';
export const PLATFORM_DEFAULT_CURRENCY = 'CNY';

/** 最小键值存储（`sessionStorage` 的结构子集；单测可注入假件）。 */
export interface KeyValueStorage {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
  removeItem(key: string): void;
}

/** 用户级偏好（任意字段都可以缺，缺 = 没设）。 */
export interface I18nPreference {
  readonly locale?: string;
  readonly timezone?: string;
  readonly currency?: string;
}

/** `GET /api/i18n/context` 的响应形状（D110）。 */
export interface I18nContextPayload {
  readonly locale?: unknown;
  readonly timezone?: unknown;
  readonly currency?: unknown;
  readonly source?: unknown;
}

/** 运行时选项（存储 / fetch / 浏览器语言都可注入，单测不碰真实浏览器与网络）。 */
export interface I18nRuntimeOptions {
  /** 偏好存储；`undefined` = 自动探测 `sessionStorage`；`null` = 仅内存。 */
  readonly storage?: KeyValueStorage | null;
  /** 上下文接口的 fetch（默认 `globalThis.fetch`）。 */
  readonly fetch?: typeof globalThis.fetch;
  /** 浏览器语言（默认 `navigator.language`；单测注入）。 */
  readonly navigatorLanguage?: string;
  /** 缺失键回调（透传给 `createI18n`）。 */
  readonly onMissingKey?: (key: string, locale: string) => void;
}

/** 壳层 i18n 运行时（同时是 Svelte store）。 */
export interface I18nRuntime {
  /** Svelte store 契约：立即回放当前值，返回取消订阅函数。 */
  subscribe(run: (value: I18nRuntime) => void): () => void;
  /** 当前 UI 语言。 */
  readonly locale: string;
  /** 当前时区（IANA）。 */
  readonly timezone: string;
  /** 当前币种（ISO 4217）。 */
  readonly currency: string;
  /** 翻译（缺失键按 `目标语言 → zh-CN → "?"`）。 */
  t(key: string, params?: Record<string, string | number>): string;
  /** 按当前时区格式化时间（可显式覆盖时区）。 */
  formatDate(iso: string, timezone?: string): string;
  /** 按当前 locale + 货币 exponent 格式化金额（可显式覆盖币种）。 */
  formatMoney(minorUnits: number, currency?: string): string;
  /** 切换语言（**只能**是已注册语言）；持久化并通知订阅者。 */
  setLocale(code: string): void;
  /** 切换时区；持久化并通知订阅者。 */
  setTimezone(timezone: string): void;
  /** 切换币种；持久化并通知订阅者。 */
  setCurrency(currency: string): void;
  /** 读取后端上下文（租户级配置），补齐未被用户 / 浏览器覆盖的级别。 */
  refreshContext(): Promise<void>;
}

/**
 * 把浏览器语言标签匹配到已注册语言。
 *
 * 精确优先（大小写不敏感），否则按主语言匹配（`en-GB` → `en-US`、`zh-Hans-CN` → `zh-CN`）；
 * 匹配不到返回 `undefined`（未注册 = 没设，落到下一级）。
 */
export function matchRegisteredLocale(tag: string | undefined): string | undefined {
  if (tag === undefined || tag.trim() === '') {
    return undefined;
  }
  const wanted = tag.trim().toLowerCase();
  const locales = registeredLocales();
  const exact = locales.find((code) => code.toLowerCase() === wanted);
  if (exact !== undefined) {
    return exact;
  }
  const primary = wanted.split('-')[0];
  if (primary === undefined || primary === '') {
    return undefined;
  }
  return locales.find((code) => code.toLowerCase().split('-')[0] === primary);
}

/**
 * 初始语言（用户偏好 → 浏览器 → 租户 → 平台默认）。
 *
 * 未注册的值一律视为「没设」—— 与后端协商规则一致（AC-S-090）。
 */
export function resolveInitialLocale(input: {
  readonly stored?: string;
  readonly navigatorLanguage?: string;
  readonly tenant?: string;
}): string {
  return (
    matchRegisteredLocale(input.stored) ??
    matchRegisteredLocale(input.navigatorLanguage) ??
    matchRegisteredLocale(input.tenant) ??
    PLATFORM_DEFAULT_LOCALE
  );
}

/** 语言选项（文件名 = 已注册语言；`label` 是语言自称）。 */
export function localeOptions(): readonly LocaleOption[] {
  return LOCALE_OPTIONS.filter((option) => isRegisteredLocale(option.code));
}

/** 校验 IANA 时区（`Intl` 自己判定，不自写正则、不查表）。 */
export function isValidTimeZone(timezone: string): boolean {
  try {
    new Intl.DateTimeFormat('en-US', { timeZone: timezone });
    return true;
  } catch {
    return false;
  }
}

/** 校验货币代码形态（语义（exponent）由 `@base/i18n` 负责，这里只挡明显的脏值）。 */
export function isValidCurrency(currency: string): boolean {
  return /^[A-Z]{3}$/.test(currency);
}

/** 探测 `sessionStorage` 是否可用（隐私模式 / 非浏览器环境）。 */
function detectStorage(): KeyValueStorage | null {
  try {
    const storage = (globalThis as { sessionStorage?: KeyValueStorage }).sessionStorage;
    if (storage === undefined) {
      return null;
    }
    const probe = `${I18N_PREFERENCE_KEY}.probe`;
    storage.setItem(probe, '1');
    storage.removeItem(probe);
    return storage;
  } catch (error) {
    // eslint-disable-next-line no-console -- 降级必须留痕，否则「切换语言不生效」无从排查
    console.warn('[i18n] sessionStorage 不可用，语言偏好只在本页内存中生效', error);
    return null;
  }
}

/** 读偏好（损坏 / 旧结构 → 空偏好，不抛错）。 */
export function readPreference(storage: KeyValueStorage | null): I18nPreference {
  if (storage === null) {
    return {};
  }
  let raw: string | null;
  try {
    raw = storage.getItem(I18N_PREFERENCE_KEY);
  } catch {
    return {};
  }
  if (raw === null || raw === '') {
    return {};
  }
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch {
    return {};
  }
  if (typeof parsed !== 'object' || parsed === null || Array.isArray(parsed)) {
    return {};
  }
  const record = parsed as Record<string, unknown>;
  const pick = (key: string): string | undefined => {
    const value = record[key];
    return typeof value === 'string' && value !== '' ? value : undefined;
  };
  return { locale: pick('locale'), timezone: pick('timezone'), currency: pick('currency') };
}

/** 把偏好写成后端认识的查询串（D116：只带用户**显式设过**的级别）。 */
export function contextQuery(preference: I18nPreference): string {
  const search = new URLSearchParams();
  if (preference.locale !== undefined) {
    search.set('locale', preference.locale);
  }
  if (preference.timezone !== undefined) {
    search.set('timezone', preference.timezone);
  }
  const text = search.toString();
  return text === '' ? '' : `?${text}`;
}

/**
 * 浏览器语言。
 *
 * 只在**真的在浏览器里**（存在 `window`）时读 `navigator.language`：
 * SSR / Node 单测里 `globalThis.navigator` 也可能存在（Node 21+），
 * 那会把「测试环境的语言」当成用户偏好，让断言到一个环境的 locale 上。
 */
function browserLanguage(): string | undefined {
  const globalWindow = (globalThis as { window?: { navigator?: { language?: string } } }).window;
  return globalWindow?.navigator?.language;
}

/**
 * 构造运行时。
 *
 * 单例入口见 `i18nRuntime()`；单测用本函数注入假存储 / 假 fetch。
 */
export function createI18nRuntime(options: I18nRuntimeOptions = {}): I18nRuntime {
  const storage = options.storage === undefined ? detectStorage() : options.storage;
  const fetchImpl: typeof globalThis.fetch =
    options.fetch ?? ((input, init) => globalThis.fetch(input, init));
  const remembered = readPreference(storage);
  const navigatorLanguage = options.navigatorLanguage ?? browserLanguage();
  // 存储里的值可能被手工改过 / 是旧版本写的：未注册或格式非法一律视为「没设」。
  const storedLocale = matchRegisteredLocale(remembered.locale);
  const storedTimezone =
    remembered.timezone !== undefined && isValidTimeZone(remembered.timezone)
      ? remembered.timezone
      : undefined;
  const storedCurrency =
    remembered.currency !== undefined && isValidCurrency(remembered.currency)
      ? remembered.currency
      : undefined;

  const initialLocale = resolveInitialLocale({
    stored: storedLocale,
    navigatorLanguage,
  });

  const i18n: I18n = createI18n({
    locale: initialLocale,
    onMissingKey: options.onMissingKey,
  });

  let timezone = storedTimezone ?? PLATFORM_DEFAULT_TIMEZONE;
  let currency = storedCurrency ?? PLATFORM_DEFAULT_CURRENCY;

  /** 用户偏好的权威副本（**只**包含用户显式设过、且合法的级别）。 */
  let preference: I18nPreference = {
    locale: storedLocale,
    timezone: storedTimezone,
    currency: storedCurrency,
  };
  /** 浏览器语言是否已经决定了语言（决定了租户配置还能不能补位）。 */
  const localeFromBrowser = matchRegisteredLocale(navigatorLanguage) !== undefined;

  const listeners = new Set<(value: I18nRuntime) => void>();
  const emit = (): void => {
    for (const listener of [...listeners]) {
      listener(runtime);
    }
  };

  const persist = (): void => {
    if (storage === null) {
      return;
    }
    try {
      storage.setItem(I18N_PREFERENCE_KEY, JSON.stringify(preference));
    } catch (error) {
      // 配额 / 隐私模式：当前页面继续可用，只是刷新后不记得。
      // eslint-disable-next-line no-console -- 降级必须留痕
      console.warn('[i18n] 写入 sessionStorage 失败，语言偏好只在内存中生效', error);
    }
  };

  const runtime: I18nRuntime = {
    subscribe(run: (value: I18nRuntime) => void): () => void {
      listeners.add(run);
      // Svelte store 契约：订阅时**立即**回放当前值（否则 `$i18n` 是 undefined）。
      run(runtime);
      return () => {
        listeners.delete(run);
      };
    },

    get locale(): string {
      return i18n.locale;
    },
    get timezone(): string {
      return timezone;
    },
    get currency(): string {
      return currency;
    },

    t(key: string, params?: Record<string, string | number>): string {
      return i18n.t(key, params);
    },
    formatDate(iso: string, zone?: string): string {
      return i18n.formatDate(iso, zone ?? timezone);
    },
    formatMoney(minorUnits: number, code?: string): string {
      return i18n.formatMoney(minorUnits, code ?? currency);
    },

    setLocale(code: string): void {
      // 未注册语言直接抛错（不静默把界面语言设成一个全是回退的值）。
      const changed = code !== i18n.locale;
      i18n.setLocale(code);
      preference = { ...preference, locale: code };
      persist();
      // 同语言重复选择不触发重渲染（与 `I18n.setLocale` 的「无变化不通知」一致）。
      if (changed) {
        emit();
      }
    },

    setTimezone(next: string): void {
      if (!isValidTimeZone(next)) {
        throw new RangeError(`invalid IANA timezone: ${next}`);
      }
      timezone = next;
      preference = { ...preference, timezone: next };
      persist();
      emit();
    },

    setCurrency(next: string): void {
      if (!isValidCurrency(next)) {
        throw new RangeError(`invalid currency code: ${next}`);
      }
      currency = next;
      preference = { ...preference, currency: next };
      persist();
      emit();
    },

    async refreshContext(): Promise<void> {
      const response = await fetchImpl(`${I18N_CONTEXT_PATH}${contextQuery(preference)}`, {
        headers: { accept: 'application/json' },
      });
      if (!response.ok) {
        throw new Error(`i18n context request failed with status ${response.status}`);
      }
      const payload = (await response.json()) as I18nContextPayload;

      // 语言：只有「用户没设、浏览器也没给出已注册语言」时，租户配置才有资格生效
      //（顺序：用户偏好 → navigator.language → 租户 → 平台默认）。
      if (preference.locale === undefined && !localeFromBrowser) {
        const tenantLocale =
          typeof payload.locale === 'string' ? matchRegisteredLocale(payload.locale) : undefined;
        if (tenantLocale !== undefined && tenantLocale !== i18n.locale) {
          i18n.setLocale(tenantLocale);
        }
      }

      // 时区：用户偏好 > 租户配置 > 平台默认（D114：渲染只在前端）。
      if (preference.timezone === undefined) {
        const tenantTimezone = payload.timezone;
        if (typeof tenantTimezone === 'string' && isValidTimeZone(tenantTimezone)) {
          timezone = tenantTimezone;
        }
      }

      // 币种：同样只补未被用户显式设过的级别。
      if (preference.currency === undefined) {
        const tenantCurrency = payload.currency;
        if (typeof tenantCurrency === 'string' && isValidCurrency(tenantCurrency)) {
          currency = tenantCurrency;
        }
      }

      emit();
    },
  };

  return runtime;
}

/** 单例（组件与 App 共用同一个实例）。 */
let singleton: I18nRuntime | undefined;

/** 取壳层 i18n 运行时（首次调用时创建）。 */
export function i18nRuntime(): I18nRuntime {
  singleton ??= createI18nRuntime();
  return singleton;
}

/** 清空单例（**仅供测试**：避免模块级状态跨用例泄漏）。 */
export function resetI18nRuntime(): void {
  singleton = undefined;
}

/**
 * 错误码 → 翻译键。
 *
 * 后端码逐字映射（`error.<CODE>`）；SDK 的客户端合成码不是后端码，
 * 放在 `common.*` 下（保持 `error.*` 与后端 code 集合一一对应）。
 */
export function errorMessageKey(code: string): string {
  if (code === 'client.NETWORK_ERROR') {
    return 'common.clientNetworkError';
  }
  if (code === 'client.INTERNAL_ERROR') {
    return 'common.clientInternalError';
  }
  return `error.${code}`;
}

/** 后端 `params` → `t()` 的插值参数（只保留标量；对象 / 数组转成字符串）。 */
export function toMessageParams(
  params: Record<string, unknown> | undefined,
): Record<string, string | number> {
  const result: Record<string, string | number> = {};
  for (const [key, value] of Object.entries(params ?? {})) {
    if (typeof value === 'string' || typeof value === 'number') {
      result[key] = value;
    } else if (value !== undefined && value !== null) {
      result[key] = JSON.stringify(value);
    }
  }
  return result;
}
