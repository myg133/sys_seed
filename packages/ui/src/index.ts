/**
 * 基础 UI 占位包。
 *
 * 本期只定义设计 token 的类型与取值入口，具体组件在后续 Sprint 实现。
 */

/** 基础色调。 */
export type Tone = 'default' | 'primary' | 'danger';

/** 全部可选色调。 */
export const TONES: readonly Tone[] = ['default', 'primary', 'danger'];

/** 色调对应的样式类名。 */
export function toneClass(tone: Tone): string {
  return `base-tone base-tone--${tone}`;
}

/** 圆角 token，单位 px。 */
export interface RadiusTokens {
  readonly small: number;
  readonly medium: number;
  readonly large: number;
}

/** 默认圆角 token。 */
export function defaultRadius(): RadiusTokens {
  return { small: 2, medium: 4, large: 8 };
}
