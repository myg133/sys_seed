/**
 * `@base/i18n` —— 前端国际化与本地化（REQ-021 / AC-S-091 · 092 · 098 · 099）。
 *
 * # 三条硬规则
 *
 * | 规则 | 判据 |
 * |---|---|
 * | `t()` 是纯函数 | 同输入同输出；不读 DOM、不读 `navigator`、不拼字符串 |
 * | 回退链 = 目标语言 → 基准语言（`zh-CN`）→ `"?"` | **不**回落到「键的最后一段」，**不**回落到完整 key |
 * | 小数位数由**货币 exponent** 决定，不是 locale | `Intl` 实测：`ja-JP + CNY` 仍是 2 位 |
 *
 * # 为什么回退到 `"?"` 而不是 key 原文
 *
 * `error.INSUFFICIENT_PERMISSION` 的最后一段就是错误码本身；把 `common.save`
 * 或 `PERMISSION` 显示给用户等于泄露实现细节。`"?"` 诚实且不泄露（裁定 #7）。
 *
 * # 缺失键的两条记录
 *
 * 裁定 #18 要求两条都做：`onMissingKey` 回调（人能看到）+ 导出的 `missingKeys()`
 * （测试 / CI 能机械断言）。`console.warn` 单独存在时测试抓不到，正是本仓反复踩过的假绿。
 *
 * # 零新依赖
 *
 * 格式化全部走内建 `Intl`（`Intl.NumberFormat` / `Intl.DateTimeFormat`），
 * 不自造千分位逻辑，也不手写时区偏移（浏览器自带 tzdb，D114）。
 */

import enUS from './locales/en-US.json';
import zhCN from './locales/zh-CN.json';

/** 文案字典：键 → 面向用户的一行文案。 */
export type MessageDict = Record<string, string>;

/**
 * 基准语言。
 *
 * 它是**唯一**的最终回退源（除 `"?"`）；「已注册语言」与「有翻译包的目录」
 * 是同一份东西，不允许两处维护（design §6.5）。
 */
export const BASE_LOCALE = 'zh-CN';

/**
 * 已注册语言 → 翻译包。
 *
 * 键集合**必须**等于 `src/locales/*.json` 的文件名（裁定 #16：后端按这些文件名
 * 判定「已注册」）。`test/i18n.test.ts` 有一条用例逐字核对目录与这张表，
 * 防止「加了语言文件但忘了登记」或反之。
 */
const BUILTIN_CATALOGS: Readonly<Record<string, MessageDict>> = {
  'zh-CN': zhCN as MessageDict,
  'en-US': enUS as MessageDict,
};

/** 语言选项（`label` 是该语言的自称，属于语言数据，不属于界面文案）。 */
export interface LocaleOption {
  readonly code: string;
  readonly label: string;
}

/** 语言切换器用的选项；文件名 = `code`。 */
export const LOCALE_OPTIONS: readonly LocaleOption[] = [
  { code: 'zh-CN', label: '简体中文' },
  { code: 'en-US', label: 'English' },
];

/** 已注册语言清单（来自翻译包目录；顺序稳定）。 */
export function registeredLocales(): readonly string[] {
  return Object.keys(BUILTIN_CATALOGS);
}

/** 该 locale 是否有翻译包。 */
export function isRegisteredLocale(code: string): boolean {
  return Object.prototype.hasOwnProperty.call(BUILTIN_CATALOGS, code);
}

/** 构造参数。 */
export interface I18nOptions {
  /** 初始语言（默认基准语言 `zh-CN`；未注册时回落到基准语言）。 */
  readonly locale?: string;
  /**
   * 注入文案（**单测 / 夹具用**）。
   *
   * 两种形状都认：① `{ 'zh-CN': {...}, 'en-US': {...} }`；② 直接给一份字典
   * `{ 'common.save': '保存' }`（视为初始语言的语言包）。
   * 列出的语言**替换**内置语言包（未列出的仍用内置）—— 这是「缺键」用例
   * （`en-US` 缺 `common.save` → 回退 `zh-CN`）唯一可靠的构造方式。
   */
  readonly messages?: Readonly<Record<string, MessageDict>> | MessageDict;
  /** 缺失键回调（默认 `console.warn` 一条）。每次缺失都会调用，**不去重**。 */
  readonly onMissingKey?: (key: string, locale: string) => void;
}

/** 国际化实例。 */
export interface I18n {
  /**
   * 当前语言。
   *
   * `setLocale` 只能设成**已注册语言**（有翻译包）；构造函数允许传任意合法
   * BCP-47 标签 —— 纯格式化（`formatDate` / `formatMoney`）需要它，
   * 例如「用 `ja-JP` 格式化 CNY」（AC-S-098）。没有翻译包时 `t()` 全走基准语言回退。
   */
  readonly locale: string;
  /** 切换语言：未注册时抛 `RangeError`；变化时**同步**通知全部订阅者。 */
  setLocale(code: string): void;
  /** 翻译；缺失时按「目标语言 → zh-CN → `?`」回退并记录。 */
  t(key: string, params?: Record<string, string | number>): string;
  /** 按 `locale` + 时区格式化时间；`timezone` 省略时用运行环境的系统时区。 */
  formatDate(iso: string, timezone?: string): string;
  /** 按 `locale` + **货币 exponent** 格式化金额（最小单位整数）。 */
  formatMoney(minorUnits: number, currency: string): string;
  /** 订阅变化（组件重新渲染用）；返回取消订阅函数。 */
  subscribe(fn: () => void): () => void;
}

/** 会话内累计的缺失键（去重）。 */
const missing = new Set<string>();

/**
 * 当前会话内累计的缺失键（完整键路径，去重）。
 *
 * 返回的是副本 —— 调用方改不动内部集合。
 */
export function missingKeys(): ReadonlySet<string> {
  return new Set(missing);
}

/** 清空缺失键集合（**测试 / 运维工具**：`missingKeys().size` 的断言需要干净起点）。 */
export function resetMissingKeys(): void {
  missing.clear();
}

/** 默认的缺失键告警。 */
function defaultOnMissingKey(key: string, locale: string): void {
  // eslint-disable-next-line no-console -- 缺失键必须人能看到（裁定 #18：回调 + 集合两条都做）
  console.warn(`[i18n] missing key "${key}" for locale "${locale}"`);
}

/** 占位符：`{name}`。 */
const PLACEHOLDER = /\{([A-Za-z0-9_]+)\}/g;

/**
 * 替换占位符。
 *
 * 参数缺失时替换成 `"?"`（与缺键一致）—— **不允许**把 `{required}` 原样显示给用户。
 */
function interpolate(text: string, params?: Record<string, string | number>): string {
  return text.replace(PLACEHOLDER, (_match: string, name: string) => {
    const value = params?.[name];
    return value === undefined ? '?' : String(value);
  });
}

/** 把注入的两种形状归一成「locale → 字典」。 */
function normalizeMessages(
  messages: I18nOptions['messages'],
  initialLocale: string,
): Record<string, MessageDict> {
  if (messages === undefined) {
    return {};
  }
  const keys = Object.keys(messages);
  const looksLikeOneDict = keys.every((key) => /^(error|common|schema|nav)\./.test(key));
  if (looksLikeOneDict) {
    return { [initialLocale]: messages as MessageDict };
  }
  return messages as Record<string, MessageDict>;
}

/** 货币 exponent 表（规格钉死，裁定 #20）。 */
const CURRENCY_EXPONENTS: Readonly<Record<string, number>> = {
  CNY: 2,
  USD: 2,
  EUR: 2,
  JPY: 0,
  KRW: 0,
};

/** 表外货币的默认小数位。 */
export const DEFAULT_CURRENCY_EXPONENT = 2;

/** 取货币的 exponent（表外默认 2）。 */
export function currencyExponent(currency: string): number {
  return CURRENCY_EXPONENTS[currency] ?? DEFAULT_CURRENCY_EXPONENT;
}

/**
 * 最小单位整数 → 主单位数值，**用整数拆分**而不是浮点乘除。
 *
 * 例如 `123456` + exponent 2 → `"1234.56"` → `1234.56`。
 * 千分位与货币符号仍交给 `Intl.NumberFormat`，这里只负责「除以 10^exponent」这一步，
 * 且用十进制字符串拆位（不做浮点乘除、不做 `toFixed`）—— AC-S-098 / TC-021-084。
 */
function minorUnitsToMajor(minorUnits: number, exponent: number): number {
  if (!Number.isSafeInteger(minorUnits)) {
    // 超出安全整数范围时，数值本身在 JS 里已经丢精度 —— 宁可报错也不渲染错的金额。
    throw new RangeError(`minor units must be a safe integer: ${minorUnits}`);
  }
  const negative = minorUnits < 0;
  const digits = Math.abs(minorUnits)
    .toString()
    .padStart(exponent + 1, '0');
  const whole = exponent === 0 ? digits : digits.slice(0, digits.length - exponent);
  const fraction = exponent === 0 ? '' : digits.slice(digits.length - exponent);
  const text = `${negative ? '-' : ''}${whole}${fraction === '' ? '' : `.${fraction}`}`;
  return Number(text);
}

/** 货币代码校验（ISO 4217 形态；语义交给 `Intl` 判定）。 */
const CURRENCY_CODE = /^[A-Z]{3}$/;

/**
 * 取货币格式化器。
 *
 * - 形态不合法（不是三位大写字母）→ **抛错**；
 * - 形态合法但平台不认识（如 `ABC`）→ 交给 `Intl` 按**代码字面**渲染
 *   （`ABC 1,234.56`），**绝不静默当成 CNY**（TC-021-085 的「回退」分支）。
 */
function moneyFormatter(locale: string, currency: string, exponent: number): Intl.NumberFormat {
  if (!CURRENCY_CODE.test(currency)) {
    throw new RangeError(`unknown currency code: ${currency}`);
  }
  try {
    return new Intl.NumberFormat(locale, {
      style: 'currency',
      currency,
      minimumFractionDigits: exponent,
      maximumFractionDigits: exponent,
    });
  } catch (error) {
    throw new RangeError(
      `unknown currency code: ${currency} (${error instanceof Error ? error.message : 'invalid'})`,
    );
  }
}

/** 日期部件（固定 `yyyy-MM-dd HH:mm:ss`，24 小时制，AC-S-094 / TC-021-049）。 */
const DATE_PARTS_OPTIONS: Intl.DateTimeFormatOptions = {
  year: 'numeric',
  month: '2-digit',
  day: '2-digit',
  hour: '2-digit',
  minute: '2-digit',
  second: '2-digit',
  hourCycle: 'h23',
};

/**
 * 格式化一个时间点。
 *
 * 时区换算**完全交给 `Intl.DateTimeFormat`**（浏览器自带 tzdb，D114）：
 * 这里只把 `formatToParts` 的部件拼成固定形状，**没有任何手写偏移**。
 *
 * @param iso       RFC 3339 UTC 字符串（如 `2026-01-15T00:00:00Z`）
 * @param timezone  IANA 时区；省略 = 运行环境的系统时区
 */
export function formatIsoDateTime(iso: string, timezone?: string): string {
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) {
    throw new RangeError(`invalid ISO datetime: ${iso}`);
  }
  const options: Intl.DateTimeFormatOptions =
    timezone === undefined ? DATE_PARTS_OPTIONS : { ...DATE_PARTS_OPTIONS, timeZone: timezone };
  // 显式 `en-US` 只用于取**数字部件**（保证 ASCII 数字与 0 补位）；
  // 用户看到的日期形状由本函数钉死为 `yyyy-MM-dd HH:mm:ss`，与 locale 无关。
  const parts = new Intl.DateTimeFormat('en-US', options).formatToParts(date);
  const part = (type: Intl.DateTimeFormatPartTypes): string =>
    parts.find((candidate) => candidate.type === type)?.value ?? '';
  return `${part('year')}-${part('month')}-${part('day')} ${part('hour')}:${part('minute')}:${part('second')}`;
}

/** 判断是否是合法的 BCP-47 语言标签（`Intl` 自己判定，不自写正则）。 */
function isValidLocaleTag(code: string): boolean {
  try {
    Intl.getCanonicalLocales(code);
    return true;
  } catch {
    return false;
  }
}

/**
 * 构造国际化实例。
 *
 * @example
 * const i18n = createI18n({ locale: 'en-US' });
 * i18n.t('common.save');                              // => "Save"
 * i18n.t('error.INSUFFICIENT_PERMISSION', { required: 'rbac:role:write' });
 * i18n.formatMoney(123456, 'CNY');                     // => "CN¥1,234.56"
 */
export function createI18n(options: I18nOptions = {}): I18n {
  const catalogs: Record<string, MessageDict> = {};
  for (const [code, dict] of Object.entries(BUILTIN_CATALOGS)) {
    catalogs[code] = dict;
  }
  const injected = normalizeMessages(options.messages, options.locale ?? BASE_LOCALE);
  for (const [code, dict] of Object.entries(injected)) {
    catalogs[code] = dict;
  }

  // 初始语言：已注册的用语言包；未注册但**合法**的 BCP-47 标签保留给格式化
  // （如 `ja-JP` 只为验证 exponent，不要求有翻译包）；非法标签回落到基准语言。
  let current = options.locale ?? BASE_LOCALE;
  if (!(current in catalogs) && !isValidLocaleTag(current)) {
    current = BASE_LOCALE;
  }

  const listeners = new Set<() => void>();
  const reportMissing = options.onMissingKey ?? defaultOnMissingKey;

  const lookup = (locale: string, key: string): string | undefined => {
    const dict = catalogs[locale];
    if (dict === undefined) {
      return undefined;
    }
    return Object.prototype.hasOwnProperty.call(dict, key) ? dict[key] : undefined;
  };

  const recordMissing = (key: string): void => {
    missing.add(key);
    reportMissing(key, current);
  };

  return {
    get locale(): string {
      return current;
    },

    setLocale(code: string): void {
      if (!(code in catalogs)) {
        throw new RangeError(`unregistered locale: ${code}`);
      }
      if (code === current) {
        return;
      }
      current = code;
      for (const listener of [...listeners]) {
        listener();
      }
    },

    t(key: string, params?: Record<string, string | number>): string {
      const direct = lookup(current, key);
      if (direct !== undefined) {
        return interpolate(direct, params);
      }
      // 目标语言缺该键：先记录，再用基准语言兜底（记录不因兜底而消失 —— AC-S-092 步骤 3）。
      recordMissing(key);
      const base = lookup(BASE_LOCALE, key);
      if (base !== undefined) {
        return interpolate(base, params);
      }
      // 两种语言都缺 → 统一占位，**不**泄露 key（AC-S-092 步骤 4/5）。
      return '?';
    },

    formatDate(iso: string, timezone?: string): string {
      return formatIsoDateTime(iso, timezone);
    },

    formatMoney(minorUnits: number, currency: string): string {
      const exponent = currencyExponent(currency);
      const major = minorUnitsToMajor(minorUnits, exponent);
      return moneyFormatter(current, currency, exponent).format(major);
    },

    subscribe(fn: () => void): () => void {
      listeners.add(fn);
      return () => {
        listeners.delete(fn);
      };
    },
  };
}

/* -------------------------------------------------------------------------- */
/* 兼容层（REQ-011 交付的 2 个函数）                                            */
/* -------------------------------------------------------------------------- */

/**
 * 取文案；未命中时回落到 `fallback`（默认回落到 key 本身）。
 *
 * @deprecated 新代码请用 `createI18n().t()`：本函数的默认回退是 **key 原文**，
 * 会向用户泄露实现细节（AC-S-092 步骤 5）。保留它是为了不破坏 REQ-011 的既有调用点。
 */
export function translate(dict: MessageDict, key: string, fallback: string = key): string {
  return dict[key] ?? fallback;
}

/** 判断错误码在字典中是否有对应文案。 */
export function hasMessage(dict: MessageDict, key: string): boolean {
  return Object.prototype.hasOwnProperty.call(dict, key);
}
