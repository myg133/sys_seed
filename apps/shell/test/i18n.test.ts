import { beforeEach, describe, expect, it, vi } from 'vitest';

import { createI18n } from '@base/i18n';
import { ApiError } from '@base/sdk';

import {
  I18N_CONTEXT_PATH,
  I18N_PREFERENCE_KEY,
  PLATFORM_DEFAULT_CURRENCY,
  PLATFORM_DEFAULT_LOCALE,
  PLATFORM_DEFAULT_TIMEZONE,
  contextQuery,
  createI18nRuntime,
  errorMessageKey,
  toMessageParams,
  isValidTimeZone,
  localeOptions,
  matchRegisteredLocale,
  readPreference,
  resolveInitialLocale,
  type KeyValueStorage,
} from '../src/lib/i18n';

/** 内存版 sessionStorage（单测不碰真实浏览器存储）。 */
function memoryStorage(initial: Record<string, string> = {}): KeyValueStorage & {
  data: Map<string, string>;
} {
  const data = new Map(Object.entries(initial));
  return {
    data,
    getItem: (key: string) => data.get(key) ?? null,
    setItem: (key: string, value: string) => {
      data.set(key, value);
    },
    removeItem: (key: string) => {
      data.delete(key);
    },
  };
}

/** 假 fetch：返回固定上下文，并记录每次请求的 URL。 */
function contextFetch(
  payload: unknown,
  status = 200,
): {
  fetch: typeof globalThis.fetch;
  calls: string[];
} {
  const calls: string[] = [];
  const impl = (async (input: RequestInfo | URL) => {
    calls.push(String(input));
    return new Response(JSON.stringify(payload), {
      status,
      headers: { 'content-type': 'application/json' },
    });
  }) as unknown as typeof globalThis.fetch;
  return { fetch: impl, calls };
}

/** 已持久化的用户偏好。 */
function storedPreference(preference: Record<string, string>): Record<string, string> {
  return { [I18N_PREFERENCE_KEY]: JSON.stringify(preference) };
}

beforeEach(() => {
  vi.restoreAllMocks();
});

describe('语言标签匹配（未注册 = 没设）', () => {
  it('精确匹配（大小写不敏感）与主语言匹配', () => {
    expect(matchRegisteredLocale('zh-CN')).toBe('zh-CN');
    expect(matchRegisteredLocale('EN-us')).toBe('en-US');
    expect(matchRegisteredLocale('en-GB')).toBe('en-US');
    expect(matchRegisteredLocale('zh-Hans-CN')).toBe('zh-CN');
  });

  it('未注册 / 空值 → undefined', () => {
    expect(matchRegisteredLocale('fr-FR')).toBeUndefined();
    expect(matchRegisteredLocale('ja-JP')).toBeUndefined();
    expect(matchRegisteredLocale('')).toBeUndefined();
    expect(matchRegisteredLocale(undefined)).toBeUndefined();
  });

  it('切换器只列已注册语言', () => {
    expect(
      localeOptions()
        .map((option) => option.code)
        .sort(),
    ).toEqual(['en-US', 'zh-CN']);
  });
});

describe('初始语言优先级：用户偏好 > navigator > 租户 > 平台默认（AC-S-099 / TC-021-095）', () => {
  it('只有 navigator.language → 用它', () => {
    expect(resolveInitialLocale({ navigatorLanguage: 'en-US' })).toBe('en-US');
  });

  it('用户偏好 + navigator 相反 → 用户偏好', () => {
    expect(resolveInitialLocale({ stored: 'zh-CN', navigatorLanguage: 'en-US' })).toBe('zh-CN');
  });

  it('navigator 未注册 + 租户已注册 → 租户', () => {
    expect(resolveInitialLocale({ navigatorLanguage: 'fr-FR', tenant: 'en-US' })).toBe('en-US');
  });

  it('全都没有 → 平台默认', () => {
    expect(resolveInitialLocale({})).toBe(PLATFORM_DEFAULT_LOCALE);
    expect(PLATFORM_DEFAULT_LOCALE).toBe('zh-CN');
  });

  it('四种结果只落在已注册语言集合内', () => {
    const results = new Set([
      resolveInitialLocale({ navigatorLanguage: 'en-US' }),
      resolveInitialLocale({ stored: 'zh-CN', navigatorLanguage: 'en-US' }),
      resolveInitialLocale({ navigatorLanguage: 'fr-FR', tenant: 'en-US' }),
      resolveInitialLocale({}),
    ]);
    expect([...results].sort()).toEqual(['en-US', 'zh-CN']);
  });
});

describe('运行时的初始语言与切换（AC-S-099）', () => {
  it('navigator=en-US 且无偏好 → en-US；t() 输出英文', () => {
    const runtime = createI18nRuntime({ storage: null, navigatorLanguage: 'en-US' });
    expect(runtime.locale).toBe('en-US');
    expect(runtime.t('common.save')).toBe('Save');
    expect(runtime.t('common.save')).not.toBe('common.save');
  });

  it('用户偏好优先于 navigator（同一键输出中文）', () => {
    const storage = memoryStorage(storedPreference({ locale: 'zh-CN' }));
    const runtime = createI18nRuntime({ storage, navigatorLanguage: 'en-US' });
    expect(runtime.locale).toBe('zh-CN');
    expect(runtime.t('common.save')).toBe('保存');
  });

  it('切换语言：订阅者立即回放当前值，setLocale 每次通知恰一次', () => {
    const runtime = createI18nRuntime({ storage: null, navigatorLanguage: 'zh-CN' });
    const seen: string[] = [];
    const unsubscribe = runtime.subscribe((value) => {
      seen.push(value.locale);
    });
    // Svelte store 契约：订阅时立即回放（否则 `$i18n` 是 undefined）。
    expect(seen).toEqual(['zh-CN']);

    runtime.setLocale('en-US');
    expect(seen).toEqual(['zh-CN', 'en-US']);
    expect(runtime.t('common.save')).toBe('Save');

    runtime.setLocale('en-US');
    expect(seen).toEqual(['zh-CN', 'en-US']);

    unsubscribe();
    runtime.setLocale('zh-CN');
    expect(seen).toEqual(['zh-CN', 'en-US']);
  });

  it('未注册语言 → 抛 RangeError，当前语言不变', () => {
    const runtime = createI18nRuntime({ storage: null, navigatorLanguage: 'zh-CN' });
    expect(() => runtime.setLocale('ja-JP')).toThrow(RangeError);
    expect(runtime.locale).toBe('zh-CN');
  });

  it('偏好写入 sessionStorage：同标签页重建（刷新）保留；新存储（新标签页）回到 navigator', () => {
    const storage = memoryStorage();
    const first = createI18nRuntime({ storage, navigatorLanguage: 'zh-CN' });
    first.setLocale('en-US');

    const raw = storage.getItem(I18N_PREFERENCE_KEY);
    expect(raw).not.toBeNull();
    expect(readPreference(storage).locale).toBe('en-US');

    // 「同标签页刷新」= 同一个 sessionStorage 重新装配
    const reloaded = createI18nRuntime({ storage, navigatorLanguage: 'zh-CN' });
    expect(reloaded.locale).toBe('en-US');

    // 「新标签页」= 空 sessionStorage（偏好不跨标签页，D99 / D112）
    const newTab = createI18nRuntime({ storage: memoryStorage(), navigatorLanguage: 'zh-CN' });
    expect(newTab.locale).toBe('zh-CN');
  });

  it('损坏 / 旧结构的偏好 → 视为没设，不抛错', () => {
    const broken = memoryStorage({ [I18N_PREFERENCE_KEY]: '{not json' });
    expect(readPreference(broken)).toEqual({});
    const runtime = createI18nRuntime({ storage: broken });
    expect(runtime.locale).toBe(PLATFORM_DEFAULT_LOCALE);
  });
});

describe('ApiError.params 驱动文案渲染（AC-S-089b / D109）', () => {
  it('403 的 params.required 被渲染进文案（非空、不含 {...}、不是 key 本身）', () => {
    const error = new ApiError({
      code: 'INSUFFICIENT_PERMISSION',
      status: 403,
      params: { required: 'rbac:role:write' },
    });
    const i18n = createI18n({ locale: 'zh-CN' });
    const text = i18n.t(errorMessageKey(error.code), toMessageParams(error.params));

    expect(text.length).toBeGreaterThan(0);
    expect(text).toContain('rbac:role:write');
    expect(text).not.toContain('{');
    expect(text).not.toContain('}');
    expect(text).not.toBe('error.INSUFFICIENT_PERMISSION');
    // 中英文两套文案确实不同（防「两个包都是中文」）
    const english = createI18n({ locale: 'en-US' }).t(
      errorMessageKey(error.code),
      toMessageParams(error.params),
    );
    expect(english).not.toBe(text);
    expect(english).not.toMatch(/[\u4e00-\u9fff]/);
  });
});

describe('时区 / 币种优先级：用户 > 租户 > 平台默认（AC-S-094 / 095）', () => {
  it('没有用户偏好 → 租户时区生效，同一 UTC 时刻渲染成纽约时间（跨日）', async () => {
    const { fetch, calls } = contextFetch({
      locale: 'en-US',
      timezone: 'America/New_York',
      currency: 'USD',
      source: 'tenant',
    });
    const runtime = createI18nRuntime({
      storage: memoryStorage(),
      navigatorLanguage: 'zh-CN',
      fetch,
    });

    // 初始：平台默认（请求尚未返回）
    expect(runtime.timezone).toBe(PLATFORM_DEFAULT_TIMEZONE);
    expect(runtime.currency).toBe(PLATFORM_DEFAULT_CURRENCY);
    expect(runtime.formatDate('2026-01-15T00:00:00Z')).toBe('2026-01-15 08:00:00');

    await runtime.refreshContext();

    expect(calls).toEqual([I18N_CONTEXT_PATH]);
    // 语言：浏览器已给出已注册语言 → 租户不覆盖（顺序：navigator > 租户）
    expect(runtime.locale).toBe('zh-CN');
    expect(runtime.timezone).toBe('America/New_York');
    expect(runtime.currency).toBe('USD');
    expect(runtime.formatDate('2026-01-15T00:00:00Z')).toBe('2026-01-14 19:00:00');
    // 界面语言是 zh-CN（浏览器优先于租户），USD 在 zh-CN 下的符号是 `US$`。
    expect(runtime.formatMoney(123456)).toBe('US$1,234.56');
  });

  it('用户时区偏好 > 租户；请求把偏好带给后端（D116，source=user 才可能出现）', async () => {
    const { fetch, calls } = contextFetch({
      locale: 'en-US',
      timezone: 'America/New_York',
      currency: 'USD',
      source: 'tenant',
    });
    const runtime = createI18nRuntime({
      storage: memoryStorage(storedPreference({ locale: 'zh-CN', timezone: 'Europe/London' })),
      navigatorLanguage: 'en-US',
      fetch,
    });

    expect(runtime.locale).toBe('zh-CN');
    expect(runtime.timezone).toBe('Europe/London');
    expect(runtime.formatDate('2026-01-15T00:00:00Z')).toBe('2026-01-15 00:00:00');

    await runtime.refreshContext();

    expect(calls[0]).toBe(`${I18N_CONTEXT_PATH}?locale=zh-CN&timezone=Europe%2FLondon`);
    // 租户的值不能覆盖用户显式偏好
    expect(runtime.timezone).toBe('Europe/London');
    expect(runtime.currency).toBe('USD');
    expect(runtime.formatDate('2026-01-15T00:00:00Z')).toBe('2026-01-15 00:00:00');
  });

  it('navigator 未注册时租户语言才生效', async () => {
    const { fetch } = contextFetch({ locale: 'en-US', timezone: 'Asia/Tokyo', currency: 'JPY' });
    const runtime = createI18nRuntime({
      storage: memoryStorage(),
      navigatorLanguage: 'fr-FR',
      fetch,
    });
    expect(runtime.locale).toBe(PLATFORM_DEFAULT_LOCALE);
    await runtime.refreshContext();
    expect(runtime.locale).toBe('en-US');
    expect(runtime.formatMoney(123456)).toBe('\u00a5123,456');
  });

  it('无用户偏好时请求不带查询串', async () => {
    const { fetch, calls } = contextFetch({ locale: 'zh-CN', timezone: 'Asia/Shanghai' });
    const runtime = createI18nRuntime({ storage: memoryStorage(), fetch });
    await runtime.refreshContext();
    expect(calls[0]).toBe(I18N_CONTEXT_PATH);
    expect(contextQuery({})).toBe('');
  });

  it('上下文接口不可用（404）→ 抛错，本地状态不变', async () => {
    const { fetch } = contextFetch({}, 404);
    const runtime = createI18nRuntime({
      storage: memoryStorage(),
      navigatorLanguage: 'zh-CN',
      fetch,
    });
    await expect(runtime.refreshContext()).rejects.toThrow(/404/);
    expect(runtime.timezone).toBe(PLATFORM_DEFAULT_TIMEZONE);
    expect(runtime.locale).toBe('zh-CN');
  });

  it('租户给了非法时区 / 非法币种 → 忽略（不把坏值渲染出来）', async () => {
    const { fetch } = contextFetch({ timezone: 'Not/AZone', currency: 'cny' });
    const runtime = createI18nRuntime({ storage: memoryStorage(), fetch });
    await runtime.refreshContext();
    expect(runtime.timezone).toBe(PLATFORM_DEFAULT_TIMEZONE);
    expect(runtime.currency).toBe(PLATFORM_DEFAULT_CURRENCY);
  });

  it('setTimezone 校验 IANA 名称；setCurrency 校验形态，并持久化', () => {
    const storage = memoryStorage();
    const runtime = createI18nRuntime({ storage, navigatorLanguage: 'zh-CN' });
    expect(() => runtime.setTimezone('Not/AZone')).toThrow(RangeError);
    expect(() => runtime.setCurrency('cny')).toThrow(RangeError);

    runtime.setTimezone('Asia/Tokyo');
    runtime.setCurrency('JPY');
    expect(isValidTimeZone('Asia/Tokyo')).toBe(true);
    expect(runtime.formatMoney(123456)).toBe('JP¥123,456');
    expect(readPreference(storage)).toEqual({ timezone: 'Asia/Tokyo', currency: 'JPY' });

    const reloaded = createI18nRuntime({ storage, navigatorLanguage: 'en-US' });
    expect(reloaded.timezone).toBe('Asia/Tokyo');
    expect(reloaded.formatMoney(123456)).toBe('¥123,456');
  });

  it('用户币种偏好优先于租户', async () => {
    const { fetch } = contextFetch({ currency: 'USD', timezone: 'Asia/Shanghai' });
    const runtime = createI18nRuntime({
      storage: memoryStorage(storedPreference({ currency: 'JPY' })),
      navigatorLanguage: 'zh-CN',
      fetch,
    });
    await runtime.refreshContext();
    expect(runtime.currency).toBe('JPY');
    expect(runtime.formatMoney(123456)).toBe('JP¥123,456');
  });
});
