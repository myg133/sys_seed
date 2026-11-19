/**
 * 令牌定义 —— **唯一允许出现颜色字面量的文件**（D95 / AC-S-073 步骤 4）。
 *
 * 变量名与亮 / 暗取值来自 `design-summary.md` §3.4.1 的**令牌契约表**（BA 已固化）：
 *
 * | CSS 变量 | 亮色 | 暗色 | 模式相关 |
 * |---|---|---|---|
 * | `--base-primary` | `#1677ff` | `#4096ff` | ✅ |
 * | `--base-primary-hover` | `#4096ff` | `#69b1ff` | ✅ |
 * | `--base-danger` | `#d4380d` | `#ff7875` | ✅ |
 * | `--base-text` | `#1f1f1f` | `#e6e6e6` | ✅ |
 * | `--base-text-secondary` | `#8c8c8c` | `#a6a6a6` | ✅ |
 * | `--base-bg` | `#ffffff` | `#141414` | ✅ |
 * | `--base-border` | `#d9d9d9` | `#303030` | ✅ |
 * | `--base-radius` | `4` | `4` | ❌ 模式无关 |
 * | `--base-font-family` | `system-ui, -apple-system, "Segoe UI", sans-serif` | 同左 | ❌ 模式无关 |
 *
 * # 为什么 `--base-radius` 不带单位
 *
 * 后端 `theme.radius` 是 **number**（默认 4），契约表要求前端取同值 ——
 * 加 `px` 会让「后端配置值」与「前端渲染值」对不上。
 *
 * # 为什么色板在运行期以 CSS 变量落到根元素
 *
 * 内联样式优先级高于样式表 → 切换即时生效、无需重渲染；
 * 组件只消费 `var(--base-xxx)`，因此**组件包里一个颜色字面量都不需要有**。
 */

/** 主色的中性兜底（品牌缺失 / 非法时写入的值）。 */
export const NEUTRAL_PRIMARY = 'currentColor';

/** 默认主色（品牌接口没给值时的亮色主色）。 */
export const DEFAULT_PRIMARY_COLOR = '#1677ff';

/** 默认圆角（与后端 `theme.radius` 的 number 一致，无单位）。 */
export const DEFAULT_RADIUS = 4;

/** 默认字体族（与后端 `theme.fontStack` 的 array 一致，join(', ')）。 */
export const DEFAULT_FONT_FAMILY = 'system-ui, -apple-system, "Segoe UI", sans-serif';

/** 亮色令牌。 */
export const LIGHT_TOKENS = {
  '--base-primary': DEFAULT_PRIMARY_COLOR,
  '--base-primary-hover': '#4096ff',
  '--base-danger': '#d4380d',
  '--base-text': '#1f1f1f',
  '--base-text-secondary': '#8c8c8c',
  '--base-bg': '#ffffff',
  '--base-border': '#d9d9d9',
  '--base-radius': String(DEFAULT_RADIUS),
  '--base-font-family': DEFAULT_FONT_FAMILY,
} as const;

/** 暗色令牌。 */
export const DARK_TOKENS = {
  '--base-primary': '#4096ff',
  '--base-primary-hover': '#69b1ff',
  '--base-danger': '#ff7875',
  '--base-text': '#e6e6e6',
  '--base-text-secondary': '#a6a6a6',
  '--base-bg': '#141414',
  '--base-border': '#303030',
  '--base-radius': String(DEFAULT_RADIUS),
  '--base-font-family': DEFAULT_FONT_FAMILY,
} as const;
