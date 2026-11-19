import { readFileSync, readdirSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

import { beforeEach, describe, expect, it, vi } from 'vitest';

import {
  BASE_LOCALE,
  LOCALE_OPTIONS,
  createI18n,
  currencyExponent,
  formatIsoDateTime,
  hasMessage,
  isRegisteredLocale,
  missingKeys,
  registeredLocales,
  resetMissingKeys,
  translate,
} from '../src/index';

/** 语言包目录（本文件在 `packages/i18n/test/` 下）。 */
const LOCALES_DIR = fileURLToPath(new URL('../src/locales/', import.meta.url));

/** 读语言包原文。 */
function readCatalog(code: string): Record<string, string> {
  return JSON.parse(readFileSync(`${LOCALES_DIR}${code}.json`, 'utf8')) as Record<string, string>;
}

/** 目录里的语言文件名（不含扩展名）。 */
function catalogFiles(): string[] {
  return readdirSync(LOCALES_DIR)
    .filter((name) => name.endsWith('.json'))
    .map((name) => name.replace(/\.json$/, ''))
    .sort();
}

/** CJK 正则（判断「英文界面里不该出现中文」）。 */
const CJK = /[\u3400-\u4dbf\u4e00-\u9fff]/;

beforeEach(() => {
  resetMissingKeys();
});

describe('语言包与键体系（AC-S-091）', () => {
  it('语言包文件名 = 已注册语言清单（裁定 #16）', () => {
    expect(catalogFiles()).toEqual(['en-US', 'zh-CN']);
    expect([...registeredLocales()].sort()).toEqual(catalogFiles());
    expect([...LOCALE_OPTIONS.map((option) => option.code)].sort()).toEqual(catalogFiles());
    expect(isRegisteredLocale('zh-CN')).toBe(true);
    expect(isRegisteredLocale('en-US')).toBe(true);
    expect(isRegisteredLocale('ja-JP')).toBe(false);
  });

  it('键格式 <domain>.<KEY>，domain 是四值枚举，错误键是大写 + 下划线', () => {
    const catalog = readCatalog(BASE_LOCALE);
    const domains = new Set(['error', 'common', 'schema', 'nav']);
    for (const key of Object.keys(catalog)) {
      const [domain, name] = key.split('.');
      expect(domains.has(domain ?? ''), `非法 domain: ${key}`).toBe(true);
      expect(name, `缺少键名: ${key}`).toBeTruthy();
      if (domain === 'error') {
        expect(name, `错误键必须是 UPPER_SNAKE: ${key}`).toMatch(/^[A-Z][A-Z0-9_]*$/);
      } else {
        expect(name, `非错误键必须是 camelCase: ${key}`).toMatch(/^[a-z][A-Za-z0-9]*$/);
      }
    }
  });

  it('两个语言包的键集合完全一致（缺键不能靠回退掩盖）', () => {
    const zh = Object.keys(readCatalog('zh-CN')).sort();
    const en = Object.keys(readCatalog('en-US')).sort();
    expect(en).toEqual(zh);
    expect(zh.length).toBeGreaterThanOrEqual(50);
  });

  it('同一语言内没有「值相同但键不同」的条目（同一语义只有一个键）', () => {
    for (const code of catalogFiles()) {
      const catalog = readCatalog(code);
      const seen = new Map<string, string>();
      for (const [key, value] of Object.entries(catalog)) {
        const previous = seen.get(value);
        expect(previous, `重复文案「${value}」：${previous} 与 ${key}`).toBeUndefined();
        seen.set(value, key);
      }
    }
  });

  it('每个值都非空，且不含未填充的占位符冲突', () => {
    for (const code of catalogFiles()) {
      const catalog = readCatalog(code);
      for (const [key, value] of Object.entries(catalog)) {
        expect(value.trim(), `空文案: ${key}`).not.toBe('');
      }
    }
  });

  it('错误键逐字等于后端 code 常量值（抽样 + 全部 UPPER_SNAKE）', () => {
    const catalog = readCatalog(BASE_LOCALE);
    // 逐个来自 crates/**/src/**/error.rs 与 problem.rs / health.rs / config.rs 的 `pub const CODE_*`
    const codes = [
      'UNAUTHENTICATED',
      'INSUFFICIENT_PERMISSION',
      'NOT_FOUND',
      'INTERNAL_ERROR',
      'SCHEMA_MODULE_NOT_FOUND',
      'SCHEMA_ENTITY_NOT_FOUND',
      'INVALID_CREDENTIALS',
      'MFA_REQUIRED',
      'MFA_INVALID',
      'ACCOUNT_LOCKED',
      'ACCOUNT_DISABLED',
      'TOKEN_EXPIRED',
      'TOKEN_REVOKED',
      'TOKEN_REUSE_DETECTED',
      'PASSWORD_POLICY_VIOLATION',
      'PASSWORD_REUSED',
      'LOGIN_RATE_LIMITED',
      'TENANT_CONTEXT_MISSING',
      'CONFLICT',
      'NOT_IMPLEMENTED',
      'ORG_CYCLE',
      'ORG_NODE_NOT_EMPTY',
      'ORG_NODE_HAS_MEMBERS',
      'ORG_CODE_DUPLICATED',
      'ORG_NODE_TYPE_INVALID',
      'INVALID_ARGUMENT',
      'SYNC_CONFLICT_PENDING',
      'SYNC_CONFLICT_STRATEGY',
      'SYNC_PUSH_FAILED',
      'BUILTIN_ROLE_PROTECTED',
      'WILDCARD_FORBIDDEN',
      'INHERITANCE_CYCLE',
      'REGISTRY_MISMATCH',
      'CONFIG_KEY_INVALID',
      'CONFIG_KEY_NOT_REGISTERED',
      'CONFIG_SCOPE_NOT_ALLOWED',
      'CONFIG_TYPE_MISMATCH',
      'CONFIG_VALUE_OUT_OF_RANGE',
      'CONFIG_VALUE_FORMAT_INVALID',
      'CONFIG_FLAG_USE_FLAG_ENDPOINT',
      'CONFIG_TENANT_RESERVED',
      'FLAG_NAME_INVALID',
      'INVALID_CONFIG_KEY',
      'INVALID_IDENTIFIER',
      'IO_ERROR',
      'DATABASE_ERROR',
      'INVALID_QUERY',
      'RECORD_NOT_FOUND',
      'RLS_POLICY_MISSING',
      'RLS_BYPASSED_BY_ROLE',
      'DATABASE_UNAVAILABLE',
      'MIGRATIONS_PENDING',
      'MIGRATION_DIRTY',
      'CONFIG_MISSING_REQUIRED',
      'CONFIG_INVALID_VALUE',
      'CONFIG_FORBIDDEN_COMBINATION',
      // 本需求规格钉死、后端 Dev 同期新增的两个码
      'MONEY_OVERFLOW',
      'FX_RATE_NOT_FOUND',
    ];
    for (const code of codes) {
      expect(catalog[`error.${code}`], `缺少键 error.${code}`).toBeTypeOf('string');
    }
    // 错误域里不应混入非后端错误码（客户端合成码在 common.*）
    expect(Object.keys(catalog).filter((key) => key.startsWith('error.'))).toHaveLength(
      codes.length,
    );
  });
});

describe('t() 的加载 / 切换 / 可逆（AC-S-099）', () => {
  it('zh-CN 渲染已知键得到该语言的值，且不是 key 本身', () => {
    const i18n = createI18n({ locale: 'zh-CN' });
    const text = i18n.t('common.save');
    expect(i18n.locale).toBe('zh-CN');
    expect(text).toBe('保存');
    expect(text).not.toBe('common.save');
    expect(text).not.toBe('Save');
  });

  it('切到 en-US 后同一键输出不同、正确，且英文界面无 CJK', () => {
    const i18n = createI18n({ locale: 'zh-CN' });
    const zh = i18n.t('common.save');
    i18n.setLocale('en-US');
    const en = i18n.t('common.save');
    expect(i18n.locale).toBe('en-US');
    expect(en).toBe('Save');
    expect(en).not.toBe(zh);
    expect(en).not.toMatch(CJK);
  });

  it('切回 zh-CN 逐字等于第一次；来回 10 次无状态残留', () => {
    const i18n = createI18n({ locale: 'zh-CN' });
    const first = i18n.t('common.save');
    i18n.setLocale('en-US');
    i18n.setLocale('zh-CN');
    expect(i18n.t('common.save')).toBe(first);
    for (let index = 0; index < 10; index += 1) {
      i18n.setLocale('en-US');
      i18n.setLocale('zh-CN');
    }
    expect(i18n.locale).toBe('zh-CN');
    expect(i18n.t('common.save')).toBe('保存');
    // 两个键都存在 → 切换全程不应累计缺失
    expect([...missingKeys()]).toEqual([]);
  });

  it('t() 是纯函数：同输入同输出，不依赖调用次数', () => {
    const i18n = createI18n({ locale: 'en-US' });
    const first = i18n.t('error.INSUFFICIENT_PERMISSION', { required: 'rbac:role:write' });
    const second = i18n.t('error.INSUFFICIENT_PERMISSION', { required: 'rbac:role:write' });
    expect(second).toBe(first);
    expect(first).toContain('rbac:role:write');
  });

  it('占位符被替换；参数缺失时替换成 "?"，不残留 {...}', () => {
    const i18n = createI18n({ locale: 'zh-CN' });
    expect(i18n.t('error.INSUFFICIENT_PERMISSION', { required: 'rbac:role:write' })).toBe(
      '当前角色缺少所需权限：rbac:role:write',
    );
    const missingParam = i18n.t('error.INSUFFICIENT_PERMISSION');
    expect(missingParam).not.toContain('{');
    expect(missingParam).toContain('?');
  });

  it('setLocale 传未注册 locale → 抛 RangeError，当前语言不变', () => {
    const i18n = createI18n({ locale: 'zh-CN' });
    expect(() => i18n.setLocale('ja-JP')).toThrow(RangeError);
    expect(i18n.locale).toBe('zh-CN');
    expect(i18n.t('common.save')).toBe('保存');
    expect(isRegisteredLocale(i18n.locale)).toBe(true);
  });

  it('订阅：一次 setLocale 恰好通知一次；取消订阅后不再通知；多订阅者都收到', () => {
    const i18n = createI18n({ locale: 'zh-CN' });
    const first = vi.fn();
    const second = vi.fn();
    const unsubscribe = i18n.subscribe(first);
    i18n.subscribe(second);

    i18n.setLocale('en-US');
    expect(first).toHaveBeenCalledTimes(1);
    expect(second).toHaveBeenCalledTimes(1);

    // 同语言再 set 一次不算变化 → 不通知
    i18n.setLocale('en-US');
    expect(first).toHaveBeenCalledTimes(1);

    unsubscribe();
    i18n.setLocale('zh-CN');
    expect(first).toHaveBeenCalledTimes(1);
    expect(second).toHaveBeenCalledTimes(2);
  });
});

describe('缺失键的回退与记录（AC-S-092）', () => {
  it('en-US 缺 common.save → 回退 zh-CN 的「保存」，不是空串、不是 key、不是「save」', () => {
    const i18n = createI18n({
      locale: 'en-US',
      messages: { 'en-US': { 'common.cancel': 'Cancel' } },
    });
    const text = i18n.t('common.save');
    expect(text).toBe('保存');
    expect(text).not.toBe('');
    expect(text).not.toBe('common.save');
    expect(text).not.toBe('save');
  });

  it('缺失被记录：missingKeys() 含完整键路径，且去重', () => {
    const i18n = createI18n({
      locale: 'en-US',
      messages: { 'en-US': { 'common.cancel': 'Cancel' } },
    });
    i18n.t('common.save');
    i18n.t('common.save');
    i18n.t('common.delete');
    const keys = missingKeys();
    expect(keys.has('common.save')).toBe(true);
    expect(keys.has('common.delete')).toBe(true);
    expect(keys.has('save')).toBe(false);
    expect(keys.size).toBe(2);
  });

  it('onMissingKey 每次缺失都调用，参数是 (key, locale)', () => {
    const onMissingKey = vi.fn();
    const i18n = createI18n({
      locale: 'en-US',
      messages: { 'en-US': {} },
      onMissingKey,
    });
    i18n.t('common.save');
    expect(onMissingKey).toHaveBeenCalledTimes(1);
    expect(onMissingKey).toHaveBeenNthCalledWith(1, 'common.save', 'en-US');
    i18n.t('common.save');
    expect(onMissingKey).toHaveBeenCalledTimes(2);
  });

  it('默认 onMissingKey 会写 console.warn（含键与 locale）', () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    const i18n = createI18n({ locale: 'en-US', messages: { 'en-US': {} } });
    i18n.t('common.save');
    expect(warn).toHaveBeenCalledTimes(1);
    expect(String(warn.mock.calls[0]?.[0] ?? '')).toContain('common.save');
    expect(String(warn.mock.calls[0]?.[0] ?? '')).toContain('en-US');
    warn.mockRestore();
  });

  it('两种语言都缺 → "?"（不回落到键的最后一段，不泄露 key）', () => {
    const i18n = createI18n({
      locale: 'en-US',
      messages: { 'en-US': {}, 'zh-CN': {} },
    });
    expect(i18n.t('common.save')).toBe('?');
    expect(i18n.t('nodot')).toBe('?');
    expect(i18n.t('common.save')).not.toContain('.');
    expect(missingKeys().has('common.save')).toBe(true);
  });

  it('缺失记录跨语言切换累计，不因 setLocale 清空', () => {
    const i18n = createI18n({
      locale: 'en-US',
      messages: { 'en-US': {}, 'zh-CN': {} },
    });
    i18n.t('common.save');
    i18n.t('common.cancel');
    i18n.setLocale('zh-CN');
    i18n.setLocale('en-US');
    expect(missingKeys().size).toBe(2);
  });
});

/**
 * `Intl` 实测表（Node v26.13.1；QA 同口径）。
 *
 * ⚠️ `¥` 有两个码点：U+00A5（半角）与 U+FFE5（全角，只有 `ja-JP + JPY` 用）；
 * `ja-JP + CNY` 的分隔符是 **NBSP U+00A0**。断言里显式写转义，肉眼看不出来。
 */
describe('formatMoney：小数位由货币 exponent 决定（AC-S-098）', () => {
  it('zh-CN × CNY → ¥1,234.56（U+00A5，2 位小数）', () => {
    const i18n = createI18n({ locale: 'zh-CN' });
    const text = i18n.formatMoney(123456, 'CNY');
    expect(text).toBe('\u00a51,234.56');
    expect(text).not.toContain('CNY');
  });

  it('en-US × CNY → CN¥1,234.56（不是单独的 ¥）', () => {
    const i18n = createI18n({ locale: 'en-US' });
    expect(i18n.formatMoney(123456, 'CNY')).toBe('CN\u00a51,234.56');
  });

  it('ja-JP × CNY → 元 + NBSP + 1,234.56（2 位，exponent 跟随币种）', () => {
    const i18n = createI18n({ locale: 'ja-JP' });
    const text = i18n.formatMoney(123456, 'CNY');
    expect(text).toBe('元\u00a01,234.56');
    expect(text.split('.')[1]).toHaveLength(2);
  });

  it('ja-JP × JPY → ￥123,456（U+FFE5 全角，0 位小数）', () => {
    const i18n = createI18n({ locale: 'ja-JP' });
    const text = i18n.formatMoney(123456, 'JPY');
    expect(text).toBe('\uffe5123,456');
    expect(text).not.toContain('.');
  });

  it('zh-CN × JPY → JP¥123,456（不是 ￥1,234）', () => {
    const i18n = createI18n({ locale: 'zh-CN' });
    expect(i18n.formatMoney(123456, 'JPY')).toBe('JP\u00a5123,456');
  });

  it('en-US × JPY → ¥123,456（U+00A5，无前缀）', () => {
    const i18n = createI18n({ locale: 'en-US' });
    expect(i18n.formatMoney(123456, 'JPY')).toBe('\u00a5123,456');
  });

  it('en-US 下 EUR / KRW 的符号与小数位（AC-S-098 的 5 币种表）', () => {
    const i18n = createI18n({ locale: 'en-US' });
    expect(i18n.formatMoney(123456, 'EUR')).toBe('\u20ac1,234.56');
    expect(i18n.formatMoney(123456, 'KRW')).toBe('\u20a9123,456');
  });

  it('exponent 表：CNY/USD/EUR = 2，JPY/KRW = 0，其余默认 2', () => {
    expect(currencyExponent('CNY')).toBe(2);
    expect(currencyExponent('USD')).toBe(2);
    expect(currencyExponent('EUR')).toBe(2);
    expect(currencyExponent('JPY')).toBe(0);
    expect(currencyExponent('KRW')).toBe(0);
    expect(currencyExponent('CHF')).toBe(2);
  });

  it('零与负金额稳定（不出现 NaN / null / undefined）', () => {
    const i18n = createI18n({ locale: 'zh-CN' });
    expect(i18n.formatMoney(0, 'CNY')).toBe('\u00a50.00');
    expect(i18n.formatMoney(-123456, 'CNY')).toBe('-\u00a51,234.56');
    expect(i18n.formatMoney(-1, 'CNY')).toBe('-\u00a50.01');
    for (const text of [
      i18n.formatMoney(0, 'CNY'),
      i18n.formatMoney(-123456, 'CNY'),
      i18n.formatMoney(-1, 'CNY'),
    ]) {
      expect(text).not.toMatch(/NaN|null|undefined/);
    }
  });

  it('大额金额用整数拆分，不丢分组（minorUnits=123456789012 → 1,234,567,890.12）', () => {
    const i18n = createI18n({ locale: 'en-US' });
    expect(i18n.formatMoney(123456789012, 'USD')).toBe('$1,234,567,890.12');
  });

  it('未知币种：形态合法时按代码字面渲染，形态不合法时抛错（不静默当 CNY）', () => {
    const i18n = createI18n({ locale: 'en-US' });
    // Intl 对形态合法的未知代码不抛错 —— 用代码字面 + 数值，绝不替换成 ¥。
    const unknown = i18n.formatMoney(123456, 'ABC');
    expect(unknown).toContain('ABC');
    expect(unknown).toContain('1,234.56');
    expect(unknown).not.toContain('\u00a5');
    // 形态不合法（小写 / 位数不对）→ 抛错，消息含原值。
    expect(() => i18n.formatMoney(123456, 'cny')).toThrow(RangeError);
    expect(() => i18n.formatMoney(123456, 'cny')).toThrow(/cny/);
    expect(() => i18n.formatMoney(123456, 'CN')).toThrow(RangeError);
  });

  it('非安全整数（JS 已丢精度）→ RangeError，不渲染错金额', () => {
    const i18n = createI18n({ locale: 'en-US' });
    expect(() => i18n.formatMoney(Number.MAX_SAFE_INTEGER + 1, 'CNY')).toThrow(RangeError);
  });
});

describe('formatDate：同一时刻在不同时区正确（AC-S-094，跨日）', () => {
  const ISO = '2026-01-15T00:00:00Z';

  it('Asia/Shanghai → 2026-01-15 08:00:00', () => {
    const i18n = createI18n({ locale: 'zh-CN' });
    expect(i18n.formatDate(ISO, 'Asia/Shanghai')).toBe('2026-01-15 08:00:00');
  });

  it('America/New_York → 2026-01-14 19:00:00（日期变了）', () => {
    const i18n = createI18n({ locale: 'zh-CN' });
    expect(i18n.formatDate(ISO, 'America/New_York')).toBe('2026-01-14 19:00:00');
  });

  it('Europe/London（1 月 = GMT）→ 2026-01-15 00:00:00', () => {
    const i18n = createI18n({ locale: 'zh-CN' });
    expect(i18n.formatDate(ISO, 'Europe/London')).toBe('2026-01-15 00:00:00');
  });

  it('Europe/London 夏令时（7 月）→ +01:00', () => {
    const i18n = createI18n({ locale: 'zh-CN' });
    expect(i18n.formatDate('2026-07-15T00:00:00Z', 'Europe/London')).toBe('2026-07-15 01:00:00');
  });

  it('三个时区的结果两两不同，且都不等于 UTC 原值', () => {
    const i18n = createI18n({ locale: 'zh-CN' });
    const shanghai = i18n.formatDate(ISO, 'Asia/Shanghai');
    const newYork = i18n.formatDate(ISO, 'America/New_York');
    const london = i18n.formatDate(ISO, 'Europe/London');
    expect(new Set([shanghai, newYork, london]).size).toBe(3);
    expect([shanghai, newYork, london]).not.toContain(ISO);
    expect(shanghai).not.toBe(newYork);
  });

  it('不传 timezone 时用系统时区（不悄悄用 UTC）', () => {
    const i18n = createI18n({ locale: 'zh-CN' });
    const systemZone = Intl.DateTimeFormat().resolvedOptions().timeZone;
    expect(i18n.formatDate(ISO)).toBe(i18n.formatDate(ISO, systemZone));
  });

  it('非法 ISO / 非法时区 → 抛错，不返回空串', () => {
    expect(() => formatIsoDateTime('not-a-date', 'UTC')).toThrow(RangeError);
    expect(() => formatIsoDateTime(ISO, 'Not/AZone')).toThrow(RangeError);
  });

  it('实现只用 Intl 取时区，没有手写偏移换算', () => {
    const source = readFileSync(fileURLToPath(new URL('../src/index.ts', import.meta.url)), 'utf8');
    expect(source).toContain('new Intl.DateTimeFormat(');
    expect(source).toContain('timeZone');
    expect(source).not.toContain('getTimezoneOffset');
    expect(source).not.toContain('3600000');
    expect(source).not.toMatch(/\+\s*8\s*\*\s*60/);
  });
});

describe('formatMoney 的实现约束（TC-021-084）', () => {
  it('用 Intl.NumberFormat，且不做手写千分位 / toFixed 浮点缩放', () => {
    const source = readFileSync(fileURLToPath(new URL('../src/index.ts', import.meta.url)), 'utf8');
    expect(source).toContain('new Intl.NumberFormat(');
    expect(source).not.toContain('toFixed(');
    expect(source).not.toContain('* 0.01');
    expect(source).not.toContain('/ 100');
    // 手写分组（每三位插逗号）的典型痕迹
    expect(source).not.toMatch(/\\d\{3\}/);
  });
});

describe('兼容层（REQ-011 交付的 translate / hasMessage）', () => {
  const DICT = {
    INSUFFICIENT_PERMISSION: '当前角色缺少所需权限',
    INVALID_CONFIG_KEY: '配置键不合法',
  };

  it('命中时返回中文文案', () => {
    expect(translate(DICT, 'INSUFFICIENT_PERMISSION')).toBe('当前角色缺少所需权限');
  });

  it('未命中时回落到默认文案（但新代码不应依赖这条回退）', () => {
    expect(translate(DICT, 'UNKNOWN_CODE')).toBe('UNKNOWN_CODE');
    expect(translate(DICT, 'UNKNOWN_CODE', '未知错误')).toBe('未知错误');
  });

  it('判断文案是否存在', () => {
    expect(hasMessage(DICT, 'INVALID_CONFIG_KEY')).toBe(true);
    expect(hasMessage(DICT, 'UNKNOWN_CODE')).toBe(false);
  });
});
