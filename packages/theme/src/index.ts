/**
 * 主题占位包。
 *
 * 依据 `api-conventions v1` C9，配置键名 camelCase、层级以 `.` 分隔，
 * 例如 `primaryColor`、`theme.radius`。
 */

/** 主题 token。 */
export interface ThemeTokens {
  /** 主色。 */
  readonly primaryColor: string;
  /** 圆角，单位 px。 */
  readonly radius: number;
  /** 字体族。 */
  readonly fontFamily: string;
}

/** 默认主题 token。 */
export function defaultTheme(): ThemeTokens {
  return {
    primaryColor: '#1677ff',
    radius: 4,
    fontFamily: 'system-ui, -apple-system, "Segoe UI", sans-serif',
  };
}

/** 主题 token 覆盖一层（未覆盖的项沿用默认值）。 */
export function mergeTheme(overrides: Partial<ThemeTokens>): ThemeTokens {
  return { ...defaultTheme(), ...overrides };
}
