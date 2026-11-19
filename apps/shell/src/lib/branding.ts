/**
 * 品牌配置的读取与应用（REQ-010 / AC-S-055；REQ-011 起**委托 `@base/theme`**）。
 *
 * # 为什么不把品牌值写进源码
 *
 * `apps/shell/**` 里任何品牌**具体值**都会让「改配置」变成「改另一个常量」。
 * 这里只保留**接口路径**与**中性兜底**：
 *
 * - 标题：接口给了就渲染，没给就留空（不回落任何旧品牌）；
 * - 主色：接口给了合法的十六进制颜色就写进 CSS 变量 `--base-primary`，
 *   否则由 `@base/theme` **自己**写 `currentColor`（中性，不是品牌兜底）。
 *
 * # 为什么委托 `@base/theme`
 *
 * 两份实现（shell 一份、theme 一份）会漂移 —— REQ-010 的既有 E2E 会因漂移而红。
 * 品牌 → CSS 变量的映射**只有一处**：`@base/theme`。
 *
 * # 为什么是同源相对路径
 *
 * 本仓**没有 CORS 层**。页面必须由 `platform-server` 的静态目录托管（同源），
 * 否则浏览器会拦掉 `fetch`，标题/主色永远不会变（R17）。
 */

import { applyBranding as applyThemeBranding, resolvePrimaryColor } from '@base/theme';
import type { Branding } from '@base/theme';

/** 品牌接口（同源相对路径）。 */
export const BRANDING_ENDPOINT = '/api/config/branding';

export type { Branding };

/** 品牌应用结果（E2E / 单测据此断言）。 */
export interface AppliedBranding {
  /** 实际写入 `document.title` 的文本（可能是空串）。 */
  readonly title: string;
  /** 实际写入 CSS 变量 `--base-primary` 的颜色（非法 / 缺失时为 `currentColor`）。 */
  readonly primaryColor?: string;
}

/** 读取品牌配置。 */
export async function fetchBranding(fetcher: typeof fetch = fetch): Promise<Branding> {
  const response = await fetcher(BRANDING_ENDPOINT, {
    headers: { accept: 'application/json' },
  });
  if (!response.ok) {
    throw new Error(`branding request failed with status ${response.status}`);
  }
  const body: unknown = await response.json();
  if (body === null || typeof body !== 'object' || Array.isArray(body)) {
    throw new Error('branding response is not a flat object');
  }
  return body as Branding;
}

/**
 * 把品牌配置应用到文档上。
 *
 * 颜色的写入**委托 `@base/theme`**（本包不再自己判断颜色格式 ——
 * 合法十六进制的判定只有一处，避免两边漂移）。
 * `doc` 可注入（单测不需要真实 DOM）；返回值是实际生效的值。
 */
export function applyBranding(branding: Branding, doc: Document = document): AppliedBranding {
  const title = typeof branding.title === 'string' ? branding.title : '';
  doc.title = title;

  applyThemeBranding(branding, { target: doc });

  return { title, primaryColor: resolvePrimaryColor(branding) };
}
