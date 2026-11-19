# REQ-011 —— Dev 自验报告：`apps/shell` 组件 props 类型补齐

- **需求**: REQ-011 前端 SDK 骨架 —— 本次只做「**shell 组件 props 类型**」这一小块
- **分支 / worktree**: `feature/REQ-011` @ `/home/myg133/Projects/system_seed/feature-REQ-011`
- **实施人**: Dev（Worker 会话）
- **日期**: 2026-11-19
- **前置**: BA 已修 `eslint.config.js`（给 `**/*.svelte` 配了 TS parser），并给出 `Can.svelte` 的参考写法
- **本次未动**: `crates/`、`packages/sdk`、`packages/auth`、`packages/theme`、`packages/hooks`、`apps/shell/src/pages/*`

---

## 1. 交付概览

| # | 内容 | 落点 |
|---|------|------|
| 1 | 5 个组件的 props 类型改为**从 `@base/ui` 导入** | `apps/shell/src/components/{Button,Card,Dialog,Input,Table}.svelte` |
| 2 | `@base/ui` **新增**视图层契约（`Slot` / 3 个回调别名 / 4 个 `*ComponentProps`） | `packages/ui/src/index.ts` |
| 3 | Table 组件改为**泛型**（`generics="Row"`），行类型从 `rows` 传到 `columns` | `apps/shell/src/components/Table.svelte` |

---

## 2. 每个组件的类型来源

| 组件 | props 类型来源 | 额外说明 |
|---|---|---|
| `Button.svelte` | `@base/ui` → **`ButtonComponentProps`** | = `ButtonProps` + `onclick` + `children` |
| `Card.svelte` | `@base/ui` → **`CardComponentProps`** | = `Omit<CardProps,'footer'>` + `footer` 插槽 + `children` |
| `Dialog.svelte` | `@base/ui` → **`DialogComponentProps`** | = `DialogProps` + `onclose` + `children` + `footer` |
| `Input.svelte` | `@base/ui` → **`InputComponentProps`** | = `InputProps` + `oninput` |
| `Table.svelte` | `@base/ui` → **`TableProps<Row>`**（泛型参数 `Row`） | 组件没有额外 props，**直接复用已有契约**，没有新增 `TableComponentProps` |

写法统一为 `const props: X = $props()`（**整体取值，不解构**）—— 解构出来的可选 props
会被 `svelte-check` 当成必填，消费方就得把每个可选值都传一遍。

类型导入一律 `import { xxxFn, type XxxProps } from '@base/ui'`（`verbatimModuleSyntax: true`
→ 类型导入带 `type` 标记；仓库既有风格见 `apps/shell/src/lib/router.ts:22`）。

---

## 3. `@base/ui` 新增了什么（**纯新增**，已有导出一字未改）

| 新增导出 | 种类 | 为什么需要 |
|---|---|---|
| `Slot` | `type`（= `svelte` 的 `Snippet`） | 组件的 `children` / `footer` 是**插槽**；`@base/ui` 已有 `CardProps.footer` 等逻辑契约，但没有「插槽」这种视图层概念 |
| `ClickHandler` | `type` | `Button` / `Dialog` 遮罩的点击回调（`(event: MouseEvent) => void`） |
| `CloseHandler` | `type` | `Dialog` 的 `onclose`（`() => void`） |
| `ValueHandler` | `type` | `Input` 的 `oninput`（拿的是**字符串**值，不是原生事件） |
| `ButtonComponentProps` | `interface` | `ButtonProps` + `onclick` + `children` |
| `InputComponentProps` | `interface` | `InputProps` + `oninput` |
| `CardComponentProps` | `interface` | `Omit<CardProps,'footer'>` + `footer` 插槽 + `children` |
| `DialogComponentProps` | `interface` | `DialogProps` + `onclose` + `children` + `footer` |

**已有导出的语义完全没动**：`ButtonProps` / `InputProps` / `CardProps` / `DialogProps` /
`TableProps` / `TableColumn` / `buttonClass` / `inputClass` / `cardClass` / `dialogClass` /
`tableClass` / `pageInfo` / `rowKeyOf` / `cellValue` … 全部保持原样，新类型一律是 `extends`
或 `Omit` 自它们。

### 为什么 `packages/ui` 里要引 `svelte` 的类型

`{@render}` 只接受 svelte 的 `Snippet`（它的返回类型带品牌标记，普通 `() => void` 过不了
svelte-check）。插槽类型必须与 `Snippet` **同型**，否则要么组件报「must be called with a
Snippet」，要么就得在每个组件里重复定义一遍 props 接口。

因此 `packages/ui/src/index.ts` 顶部加了 **`import type { Snippet } from 'svelte';`**：

- 是 **`import type`**（纯类型，编译后**零运行时依赖**）→ rev 2 裁定 #7
  「组件库发逻辑，宿主应用发视图」的**运行时**依赖方向没有变；
- `Slot` 只是 `Snippet` 的别名，把 svelte 的概念**收敛在一个名字**上，
  组件里写 `Slot` 即可，不必每处都写 `import('@base/svelte')`。

---

## 4. 实际跑出的数字（原文）

### 4.1 `svelte-check`

```console
$ cd apps/shell && npx svelte-check --tsconfig ./tsconfig.json
Loading svelte-check in workspace: /home/myg133/Projects/system_seed/feature-REQ-011/apps/shell
Getting Svelte diagnostics...

svelte-check found 0 errors and 0 warnings
```

### 4.2 `eslint`

```console
$ pnpm exec eslint .
（无任何输出 —— eslint 只在有问题时打印）
ESLINT_EXIT=0
```

### 4.3 `pnpm -r check`（`tsc --noEmit`）

```console
$ pnpm -r check
packages/schema check: Done
packages/i18n check: Done
packages/hooks check: Done
packages/sdk check: Done
packages/theme check: Done
packages/auth check: Done
packages/ui check: Done
apps/shell check: svelte-check found 0 errors and 0 warnings
apps/shell check: Done
CHECK_EXIT=0
```

### 4.4 `pnpm -r test`

```console
$ pnpm -r test
packages/i18n    test:  Tests  3 passed (3)
packages/hooks   test:  Tests  17 passed (17)
packages/schema  test:  Tests  3 passed (3)
packages/sdk     test:  Tests  30 passed (30)
packages/theme   test:  Tests  12 passed (12)
packages/auth    test:  Tests  22 passed (22)
packages/ui      test:  Tests  14 passed (14)
apps/shell       test:  Tests  22 passed (22)
TEST_EXIT=0
```

**合计 123 passed / 0 failed**（8 个包 / 12 个测试文件）。

> ⚠️ `apps/shell` 的 vitest **没有**渲染 `.svelte` 组件的用例（现有 4 个测试文件只测
> `nav` / `router` / `branding` / `can` 四个 `.ts` 模块）—— 组件本身目前**只有类型层
> 的保护**（svelte-check），没有运行时断言。这不是本次引入的，是既有状态。

### 4.5 附加（不在要求内，但顺手确认）

```console
$ pnpm exec prettier --check packages/ui/src/index.ts apps/shell/src/components/*.svelte
All matched files use Prettier code style!
```

---

## 5. 类型是真的生效了吗（自证）

`svelte-check` 报 0 错误可能只是「什么都没检查」。我临时放了一个探针组件
（`apps/shell/src/components/__TypeProbe.svelte`，**验证后已删除**），故意传错：

```console
Error: Type '"bogus"' is not assignable to type 'ButtonVariant | undefined'. (ts)
Error: Type '"datetime"' is not assignable to type 'InputType | undefined'. (ts)
Error: Property 'name' does not exist on type '{ id: number; }'. (ts)
```

第三条尤其关键：`rows={[{ id: 1 }]}` + `columns={[{ render: (row) => row.name }]}`
里的 `row` 被正确推断成 `{ id: number }` → 说明 **Table 的泛型 `Row` 真的从 `rows`
传到了 `columns`**，行类型不是 `any`。删除探针后 `svelte-check` 回到 0 errors / 0 warnings。

---

## 6. 遇到的问题（**不只报好消息**）

### P1 —— `@base/ui` 的 `svelte` 类型是「幽灵依赖」

`packages/ui/package.json` 的 `dependencies` 只有 `@base/theme`，**没有 `svelte`**；
`svelte` 是**仓库根**的 devDependency。pnpm 严格 node_modules 下
`packages/ui/node_modules/` 只有 `@base`，TS 靠**向上走到根 `node_modules/svelte`**
才解析到类型。

- 实测**当前可用**：`pnpm --filter @base/ui check`（`tsc --noEmit`）通过；
- 风险：`@base/ui` 现在是 `private: true`，只在工作区内用，所以**当下没有影响**；
  但一旦它被单独安装 / 发布到别处，`import type { Snippet } from 'svelte'` 会解析失败。
- **没有自行加依赖** —— 本期是「零新依赖」约束，加 `svelte` 到 `packages/ui` 的
  `dependencies` 属于**改依赖语义**，超出本次授权范围，请 BA / 负责依赖的棒拍板。

### P2 —— `CardProps.footer`（布尔）与组件的 `footer`（插槽）**同名不同义**

逻辑契约里 `footer?: boolean` 是「有没有页脚」的开关，而组件的 `footer` 是**页脚内容**
（`{@render props.footer()}`）。两者同名，不能直接 `extends`（TS 会报不兼容的成员覆盖）。
处理：`CardComponentProps extends Omit<CardProps, 'footer'>`，把 `footer` 重新声明成插槽。
**逻辑契约的 `CardProps.footer` 没动**，`cardClass()` 的行为不变。

### P3 —— Table 的 `total` 表达式在有类型后**编译不过**（已等价改写）

原写法：

```ts
(props.total ?? 0) > 0 ? props.total : props.rows.length
```

`props.total` 现在是 `number | undefined`，三元里**没有被收窄**（`??` 的结果和
`props.total` 是两个表达式）→ `pageInfo(..., total: number)` 报
`number | undefined`。改成：

```ts
const total = $derived(
  props.total !== undefined && props.total > 0 ? props.total : props.rows.length,
);
```

**语义完全等价**（`undefined` 或 `0` 都退回 `rows.length`；`> 0` 用 `total`），
只是把「收窄」写成了同一条判断链。

### P4 —— `TableProps.rows` / `columns` 是**必填**（沿用已有契约，不是本次新增）

`@base/ui` 原本就把 `rows` / `columns` 写成必填，本次**没有加**新的必填项。
现有消费方 `apps/shell/src/pages/ProtectedRoles.svelte` 两个都传了 → 编译通过。
但这意味着「不传 rows 的表格」目前**用不了**（组件内部要 `props.rows.length`）。
如果产品上需要「无数据表格」这种用法，得由 BA 明确后再改契约 —— **不在本次范围**。

### P5 —— `Can.svelte` 与本次的规则**不一致**（按任务要求**没动**）

`apps/shell/src/components/Can.svelte` 仍然在**组件本地**定义 `interface CanProps`
（BA 的参考实现）。原因是 `Can` 的契约在 `@base/auth`（`can()` / `AuthState`），
`@base/ui` 里没有对应物。任务明确「`Can.svelte` 已完成，不用动」→ **保持原样**。
如果希望统一到「props 类型也进契约包」，需要 BA 决定 `CanProps` 放 `@base/auth`
还是 `@base/ui` —— **留给 BA 裁定**。

### P6 —— `apps/shell/src/lib/handlers.ts` 的文件头注释**已过时**（本次没改）

该注释仍写着「`.svelte` 脚本块只能写 **JS 语法**：仓库根的 eslint 配置没有给 `.svelte`
配 TypeScript 解析器」—— BA 修完 eslint 后这句话**已经不成立**，而它正是本次问题的
**成因描述**，留着会误导下一个开发者。

本次**没有改它**（不在「补 props 类型」的范围内，也不想顺手扩大 diff）。
建议后续单独一条改动：把这段改成「带参数的处理器放在 `.ts` 里，是为了在模板里拿到
**上下文类型**（`strict` 下模板内联箭头函数没有注解会报 implicit any）」——
**理由仍然成立，但理由换了**。

### P7 —— 页面级 props 仍是 `Record<string, any>`

`apps/shell/src/pages/*.svelte` 的 `const props = $props()` **没有类型**（本次范围只到
组件层）。所以 `<Table rows={props.roles}>` 传的是 `any` → Table 的 `Row` 被推断成 `any`
（泛型仍然工作，见 §5 的探针）。页面 props 补类型是**下一步**（属于「页面契约」，
与本次的「组件契约」是两层）。

---

## 7. 自查清单（对照任务硬性要求）

| 要求 | 结论 |
|---|---|
| 1. 类型从 `@base/ui` 导入（或 `svelte` 的 `Snippet`），不在组件里重复定义 props 接口 | ✅ 5 个组件全部 `import { fn, type XxxProps } from '@base/ui'`；组件内**零**本地 props 接口 |
| 2. 不改 `packages/ui` 已有导出语义（只允许新增） | ✅ 已有 20 个导出**一字未改**，只新增 8 个（§3） |
| 3. 不给 props 加必填性要求，消费方不能因此编译失败 | ✅ 新增的 props **全部可选**（`?`）；唯一必填的 `rows`/`columns` 是**沿用** `TableProps` 原契约；`pnpm -r check` 绿 |
| 4. 可选 props 用 `?` 标注 | ✅ 全部新增成员都是 `readonly x?: T` |
| 5. `verbatimModuleSyntax` → 类型导入必须 `import type` | ✅ `@base/ui` 里是 `import type { Snippet } from 'svelte'`；组件里用行内 `type` 标记（`import { fn, type X } from '@base/ui'`）——TS 在 `verbatimModuleSyntax` 下**认行内 `type` 标记**，`tsc --noEmit` 已实测通过 |
| 6. 不改 `crates/` / `packages/{sdk,auth,theme,hooks}` | ✅ 只碰了 `packages/ui/src/index.ts`（**仅新增类型**）+ `apps/shell/src/components/` 5 个文件 + 本报告 |

---

## 8. 提交与后续

- 提交：`[Dev] 补齐 shell 组件的 props 类型 (关联: REQ-011)`，**只在 `feature/REQ-011`**，
  **没有合到 `develop`**。
- 工作区里**别的棒**留下的未提交改动（`AGENTS.md`、`eslint.config.js`、
  `apps/shell/src/components/Can.svelte`、`capabilities/index.json`、
  `crates/xtask/src/catalog.rs`）**没有被我卷进这次提交** —— 只 `git add` 了我自己动的
  6 个文件 + 本报告。
- 待 BA 拍板：P1（`@base/ui` 的 `svelte` 幽灵依赖）、P5（`CanProps` 放哪个包）。
