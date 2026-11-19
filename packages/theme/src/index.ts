/**
 * @base/theme —— 设计令牌（REQ-011 / AC-S-073 / AC-S-074）。
 *
 * # 令牌怎么落地
 *
 * 令牌以 **CSS 变量**写到根元素的 inline style：
 * inline style 优先级高于样式表 → 切换即时生效、无需重渲染；
 * 组件只消费 `var(--base-xxx)` → **组件包里一个颜色字面量都不需要有**。
 *
 * # 三条约定
 *
 * | 约定 | 说明 |
 * |---|---|
 * | 变量名与取值 | 来自 `design-summary.md` §3.4.1 的令牌契约表（**精确 9 个**） |
 * | 暗色模式 | `data-theme="dark"` + `color-scheme: dark` + 换一组颜色变量；**可逆** |
 * | 品牌主色 | 来自 `GET /api/config/branding`；**只接受合法十六进制**；非法 / 无品牌 → 主题包**自己**写 `currentColor` |
 *
 * # 为什么品牌兜底由主题包写
 *
 * 兜底值写在**宿主样式表**里是不够的：宿主没有 `:root{--base-primary:currentColor}`
 * 时，读到的是**空串**而不是 `currentColor` → 预期不成立。
 * 所以主题包在品牌非法时**自己**把 `--base-primary` 写成 `currentColor`。
 *
 * # 注意：切换模式会重写 `--base-primary`
 *
 * 品牌主色覆盖在令牌之上；切换模式会把 `--base-primary` 写回该模式的取值，
 * 因此**宿主在切换模式后需要重新应用一次品牌**（`applyBranding`）。
 *
 * 依据 `api-conventions v1` C9，配置键名 camelCase、层级以 `.` 分隔，
 * 例如 `primaryColor`、`theme.radius`。
 */

import {
  DARK_TOKENS,
  DEFAULT_FONT_FAMILY,
  DEFAULT_PRIMARY_COLOR,
  DEFAULT_RADIUS,
  LIGHT_TOKENS,
  NEUTRAL_PRIMARY,
} from './tokens';

/** 主题模式。 */
export type ThemeMode = 'light' | 'dark';

/** 令牌值表（CSS 变量名 → 取值；**精确 9 个**）。 */
export interface ThemeTokenValues {
  /** 主色。 */
  readonly '--base-primary': string;
  /** 主色的悬停态。 */
  readonly '--base-primary-hover': string;
  /** 危险色。 */
  readonly '--base-danger': string;
  /** 正文色。 */
  readonly '--base-text': string;
  /** 次要文字色。 */
  readonly '--base-text-secondary': string;
  /** 背景色。 */
  readonly '--base-bg': string;
  /** 边框色。 */
  readonly '--base-border': string;
  /** 圆角（无单位，与后端 `theme.radius` 的 number 一致）。 */
  readonly '--base-radius': string;
  /** 字体族。 */
  readonly '--base-font-family': string;
}

/** 令牌写入目标（默认 `document`）。 */
export interface ThemeTarget {
  readonly documentElement: {
    readonly style: { setProperty(name: string, value: string): void };
    setAttribute(name: string, value: string): void;
    getAttribute?(name: string): string | null;
  };
}

/** 应用主题的选项。 */
export interface ThemeOptions {
  /** 写入目标（默认 `document`）。 */
  readonly target?: ThemeTarget;
}

/** 品牌配置（扁平 map）。 */
export type Branding = Record<string, unknown>;

/** 主题 token（取值层面的形状）。 */
export interface ThemeTokens {
  /** 主色。 */
  readonly primaryColor: string;
  /** 圆角（无单位，与后端 `theme.radius` 的 number 一致）。 */
  readonly radius: number;
  /** 字体族。 */
  readonly fontFamily: string;
}

/** 只接受十六进制字面量（避免把任意 CSS 注入自定义属性）。 */
const HEX_COLOR = /^#(?:[0-9a-fA-F]{3}|[0-9a-fA-F]{6})$/;

/** 解析写入目标。 */
function resolveTarget(options?: ThemeOptions): ThemeTarget {
  return options?.target ?? document;
}

/** 取某个模式的令牌值表。 */
export function themeTokens(mode: ThemeMode): ThemeTokenValues {
  return mode === 'dark' ? DARK_TOKENS : LIGHT_TOKENS;
}

/**
 * 应用令牌：把令牌值写到根元素的 CSS 变量上。
 *
 * 写入的变量名与取值**逐字**来自令牌契约表 —— 消费方按变量名取用。
 */
export function applyTheme(tokens: ThemeTokenValues, options?: ThemeOptions): void {
  const root = resolveTarget(options).documentElement;
  for (const [name, value] of Object.entries(tokens)) {
    root.style.setProperty(name, value);
  }
}

/**
 * 切换主题模式。
 *
 * 写 `data-theme` + `color-scheme`，并换成该模式的一组颜色变量（**可逆**）。
 */
export function setThemeMode(mode: ThemeMode, options?: ThemeOptions): void {
  const root = resolveTarget(options).documentElement;
  root.setAttribute('data-theme', mode);
  root.style.setProperty('color-scheme', mode);
  applyTheme(themeTokens(mode), options);
}

/** 取当前模式（按根元素的 `data-theme` 属性；没有则视为亮色）。 */
export function currentMode(options?: ThemeOptions): ThemeMode {
  const root = resolveTarget(options).documentElement;
  const attribute = root.getAttribute?.('data-theme');
  return attribute === 'dark' ? 'dark' : 'light';
}

/**
 * 解析品牌主色。
 *
 * 只接受合法十六进制（`#rgb` / `#rrggbb`）；
 * 非法 / 缺失 → 中性兜底 `currentColor`（**不依赖宿主样式表**）。
 */
export function resolvePrimaryColor(branding: Branding): string {
  const raw = branding['primaryColor'];
  if (typeof raw !== 'string') {
    return NEUTRAL_PRIMARY;
  }
  const value = raw.trim();
  return HEX_COLOR.test(value) ? value : NEUTRAL_PRIMARY;
}

/**
 * 应用品牌配置。
 *
 * - 主色 → `--base-primary`（非法 / 缺失时写 `currentColor`）；
 * - `theme.radius`（number）→ `--base-radius`；
 * - `theme.fontStack`（array）→ `--base-font-family`。
 */
export function applyBranding(branding: Branding, options?: ThemeOptions): void {
  const root = resolveTarget(options).documentElement;
  root.style.setProperty('--base-primary', resolvePrimaryColor(branding));

  const radius = branding['theme.radius'];
  if (typeof radius === 'number' && Number.isFinite(radius)) {
    root.style.setProperty('--base-radius', String(radius));
  }

  const fontStack = branding['theme.fontStack'];
  if (Array.isArray(fontStack)) {
    const items = fontStack.filter((item): item is string => typeof item === 'string');
    if (items.length === fontStack.length && items.length > 0) {
      root.style.setProperty('--base-font-family', items.join(', '));
    }
  }
}

/** 默认主题 token。 */
export function defaultTheme(): ThemeTokens {
  return {
    primaryColor: DEFAULT_PRIMARY_COLOR,
    radius: DEFAULT_RADIUS,
    fontFamily: DEFAULT_FONT_FAMILY,
  };
}

/** 主题 token 覆盖一层（未覆盖的项沿用默认值）。 */
export function mergeTheme(overrides: Partial<ThemeTokens>): ThemeTokens {
  return { ...defaultTheme(), ...overrides };
}
