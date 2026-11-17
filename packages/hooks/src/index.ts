/**
 * 通用状态钩子占位包。
 *
 * 本期只提供与框架无关的最小状态容器，Svelte 5 的响应式封装在后续 Sprint 补齐。
 */

/** 布尔开关状态。 */
export interface Toggle {
  /** 当前值。 */
  readonly value: boolean;
  /** 取反。 */
  toggle(): void;
  /** 设为指定值。 */
  set(next: boolean): void;
}

/** 创建布尔开关状态。 */
export function createToggle(initial = false): Toggle {
  let current = initial;
  return {
    get value(): boolean {
      return current;
    },
    toggle(): void {
      current = !current;
    },
    set(next: boolean): void {
      current = next;
    },
  };
}
