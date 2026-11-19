/**
 * 组件事件处理器的**类型化工厂**。
 *
 * # 为什么带参数的处理函数放在 `.ts` 里
 *
 * `.svelte` 在 TS 模式下**不能**声明带类型的参数 ——
 * `strict`（含 `noImplicitAny`）会把没注解的参数判成错误：
 * 模板里的行内箭头函数没有上下文类型 → 报「implicitly has an 'any' type」。
 *
 * 所以：**带参数的处理函数一律在 `.ts` 模块里造**（那里是全量类型检查的），
 * `.svelte` 只负责把工厂的产物接到模板上 ——
 * 组件里出现的箭头函数要么**没有参数**，要么参数由这里的工厂提供上下文类型。
 *
 * （注：根 `eslint.config.js` 现已给 `.svelte` 文件配了 TypeScript 解析器，
 * 所以 `.svelte` 的脚本块**可以**写 TS 类型标注了 —— 但上面那条
 * 「模板内联箭头函数没有上下文类型」的限制**依然成立**，本工厂仍然必要。）
 */

/** 输入值回调（输入框）。 */
export type ValueSetter = (value: string) => void;

/**
 * 造一个「把输入值写回状态」的处理器。
 *
 * `apply` 的参数类型由本模块提供 → 组件里的箭头函数能拿到上下文类型。
 */
export function createValueSetter(apply: (value: string) => void): ValueSetter {
  return (value) => {
    apply(value);
  };
}

/** 分页回调。 */
export type PageChangeHandler = (page: number) => void;

/**
 * 造一个「先更新本地分页状态，再通知外部」的处理器。
 *
 * @param setPage 更新本地页码（一般同时更新框架响应式状态）
 * @param notify  通知外部（可选）
 */
export function createPageChangeHandler(
  setPage: (page: number) => void,
  notify?: (page: number) => void,
): PageChangeHandler {
  return (page) => {
    setPage(page);
    notify?.(page);
  };
}

/** 键盘事件处理器。 */
export type KeyHandler = (event: KeyboardEvent) => void;

/**
 * 造一个「按 Esc 关闭」的键盘处理器。
 *
 * @param shouldClose 此刻是否应该响应 Esc（读响应式状态）
 * @param onClose     关闭动作
 */
export function createEscapeHandler(shouldClose: () => boolean, onClose: () => void): KeyHandler {
  return (event) => {
    if (shouldClose() && event.key === 'Escape') {
      onClose();
    }
  };
}
