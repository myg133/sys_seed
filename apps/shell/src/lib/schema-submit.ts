/**
 * 表单提交闸门（REQ-012 / AC-S-083 步骤 3、步骤 5）。
 *
 * # 为什么单拉一层
 *
 * 「校验失败**不发请求**」是这条 AC 的核心判据，但它原本写在 `SchemaForm.svelte` 的
 * 事件处理器里 —— 组件单测走 `svelte/server` 的 SSR 渲染，**点不到按钮**，于是这条
 * 判据没有任何组件级用例守着。QA 的 L-5 证据：把 `validateForm` 变异成「恒返回空
 * 映射」后 `apps/shell` 的 10 条用例仍全绿 —— 说明这一层是**假绿**。
 *
 * 现在闸门是纯函数：组件在 `handleSubmit` 里调它，测试也可以直接调它，
 * 断言「必填留空 → `send` 一次都没被调用」。
 *
 * # 语义
 *
 * - 校验不通过 → `{ errors, sent: false }`，**不调用** `send`（渲染层据此不发请求）；
 * - 校验通过 → 调用 `send(body)` 一次，`body` 只含元数据声明的可写字段
 *   （`readonly` 不提交，也不夹带渲染器内部状态）。
 */

import { prepareSubmit, type EntityMeta } from '@base/schema';

/** 闸门结果。 */
export interface SubmitGateResult {
  /** 字段级错误（`{}` = 校验通过）。 */
  errors: Record<string, string>;
  /** 是否把提交体交给了写请求（`false` = 一个请求都没发）。 */
  sent: boolean;
}

/**
 * 校验后再提交。
 *
 * @param meta 实体元数据（后端已按权限过滤）
 * @param values 表单当前值（键是字段 key）
 * @param send 真正发请求的动作（**只有校验通过才会被调用**）
 */
export function gateSubmit(
  meta: EntityMeta,
  values: Record<string, unknown>,
  send: (body: Record<string, unknown>) => void,
): SubmitGateResult {
  const prepared = prepareSubmit(meta, values);
  if (prepared.body === undefined) {
    return { errors: prepared.errors, sent: false };
  }
  send(prepared.body);
  return { errors: {}, sent: true };
}
