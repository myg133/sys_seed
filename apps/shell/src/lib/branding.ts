/**
 * 品牌配置的读取与应用（REQ-010，AC-S-055 / D61 / D66）。
 *
 * # 为什么不把品牌值写进源码
 *
 * `apps/shell/**` 里任何品牌**具体值**都会让「改配置」变成「改另一个常量」。
 * 这里只保留**接口路径**与**中性兜底**：
 *
 * - 标题：接口给了就渲染，没给就留空（不回落任何旧品牌）；
 * - 主色：接口给了合法的十六进制颜色就写进 CSS 变量 `--base-primary`，
 *   否则保持 `app.css` 里的中性兜底（`currentColor`）。
 *
 * # 为什么是同源相对路径
 *
 * 本仓**没有 CORS 层**。页面必须由 `platform-server` 的静态目录托管（同源），
 * 否则浏览器会拦掉 `fetch`，标题/主色永远不会变（R17）。
 */

/** 品牌接口（同源相对路径）。 */
export const BRANDING_ENDPOINT = '/api/config/branding';

/** 品牌响应的形状：扁平 map（D61）。 */
export type Branding = Record<string, unknown>;

/** 主色只接受十六进制字面量（避免把任意 CSS 注入自定义属性）。 */
const HEX_COLOR = /^#[0-9a-fA-F]{3,8}$/;

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

/** 品牌应用结果（E2E / 单测据此断言）。 */
export interface AppliedBranding {
  /** 实际写入 `document.title` 的文本（可能是空串）。 */
  readonly title: string;
  /** 实际写入 CSS 变量 `--base-primary` 的颜色；没写时为 `undefined`。 */
  readonly primaryColor?: string;
}

/**
 * 把品牌配置应用到文档上。
 *
 * `doc` 可注入（单测不需要真实 DOM）；返回值是实际生效的值。
 */
export function applyBranding(branding: Branding, doc: Document = document): AppliedBranding {
  const title = typeof branding.title === 'string' ? branding.title : '';
  doc.title = title;

  const rawColor = branding.primaryColor;
  const primaryColor =
    typeof rawColor === 'string' && HEX_COLOR.test(rawColor.trim()) ? rawColor.trim() : undefined;
  if (primaryColor !== undefined) {
    doc.documentElement.style.setProperty('--base-primary', primaryColor);
  }

  return { title, primaryColor };
}
