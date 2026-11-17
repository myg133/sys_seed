/**
 * 国际化占位包。
 *
 * 依据 `api-conventions v1` C6：后端只返回稳定错误码，面向用户的中文文案
 * 由前端 i18n 渲染，因此这里以「错误码 → 文案」为基本映射单位。
 */

/** 文案字典：错误码 → 面向用户的中文文案。 */
export type MessageDict = Record<string, string>;

/** 取文案；未命中时回落到 `fallback`（默认回落到 key 本身）。 */
export function translate(dict: MessageDict, key: string, fallback: string = key): string {
  return dict[key] ?? fallback;
}

/** 判断错误码在字典中是否有对应文案。 */
export function hasMessage(dict: MessageDict, key: string): boolean {
  return Object.prototype.hasOwnProperty.call(dict, key);
}
