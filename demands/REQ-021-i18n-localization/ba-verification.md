# REQ-021 BA 独立验证记录

- **验证者**: myg133-ba-1（BA，**不是** Dev 自证，也不是 QA 报告的复述）
- **时间**: 2026-11-19 21:15
- **对象**: `feature/REQ-021` 工作区（**未提交**）
- **基线**: `develop` @ `88b7ca3`（REQ-012 合并点）

> **为什么要独立验**: 前几轮吃过「Dev 自证 → QA 复核 → 才发现漏」的亏。
> BA 自己跑一遍，**不依赖任何人的结论**。

---

## 1. 后端测试（逐 crate，`--all-targets --include-ignored`）

命令形态（**只有一个 `--`**；`--include-ignored` 与 `scripts/test-integration.sh:149` 一致）：

```bash
for c in platform-core platform-config platform-iam platform-rbac platform-org \
         platform-db platform-schema platform-server sandbox; do
  cargo test -p $c --all-targets -- --test-threads=2 --include-ignored
done
```

| crate | passed | failed | ignored |
|---|---|---|---|
| platform-core | 127 | 0 | 0 |
| platform-config | 83 | 0 | 0 |
| platform-iam | 58 | 0 | 0 |
| platform-rbac | 78 | 0 | 0 |
| platform-org | 91 | 0 | 0 |
| platform-db | 46 | 0 | 0 |
| platform-schema | 20 | 0 | 0 |
| platform-server | 94 | 0 | 0 |
| sandbox | 33 | 0 | 0 |
| **合计** | **630** | **0** | **0** |

**0 ignored** —— 所有 `#[ignore]` 的真实库用例都**真跑**了，没有一条被跳过。

## 2. 前端测试

| 包 | 结果 |
|---|---|
| `@base/i18n` | **43 passed (43)** |
| `@base/sdk` | **36 passed + 2 skipped** → **`pnpm -r build` 后重跑 = 38 passed / 0 skipped** |
| `@base/shell` | **127 passed (127)** |
| **合计** | **208 passed / 0 skipped** |

> 那 2 条 skip 是 `describe.skipIf(!built)`（REQ-011 的既有模式）——
> **不是**因为测试有问题，是 `packages/*/dist` 不存在。构建后**全过**。
> ⚠️ 这正是「`skip` 当通过」的一个变种：**skip 本身不说明问题，skip 之后要真跑一遍**。

## 3. E2E（**干净环境**，Playwright 自己的 webServer 起服务）

```bash
export DATABASE_URL_TEST="postgres://base:base@127.0.0.1:5432/base_test"
npx playwright test e2e/
```

**20 passed / 0 failed / 0 skipped（40.7s）**

> ⚠️ **首次跑是 13 failed** —— 根因**不是**前端坏了，而是
> `scripts/e2e-server.sh` **不注入 JWT 私钥与 IAM 密钥** → 服务在启动自检失败 →
> 页面收到空响应 → SDK 报 `client.INTERNAL_ERROR`（「响应无法解析」）。
> **已修脚本**（现生成临时密钥），修后干净环境 **20/20 全过**。
> 详见 [TD-018](../../plan/tech-debt.md)。

## 4. 「零中文 title」——**BA 自己写的独立扫描**（不依赖 Dev 的测试）

脚本：`/tmp/ba_scan.py`（临时文件，未入库）

- 扫描范围：**`crates/` + `modules/` 共 117 个 `.rs`**
  （⚠️ **必须含 `modules/`** —— 第一轮漏了这儿，漏掉 12 处）
- 匹配方式：`title:` 字段 + `ProblemDetails::from_context(code, "…")` 的第 2 参数
  —— **必须跨行**（那 12 处全是跨行的，单行正则会漏）
- 剥注释后再匹配
- **阳性对照**：跨行 `from_context` ✓ / `title:` 字段 ✓（两个探针都命中）

**结果：命中数 = 0**

> ⚠️ 第一版探针写错了（用了 `p(` 而非 `from_context(`）→ 阳性对照失败 →
> **是我的探针错，不是实现有问题**。改对后通过。
> 这正说明**阳性对照是必需的**：没有它，探针坏了你也不知道。

## 5. `TITLES` 表质量（BA 独立解析）

| 检查 | 结果 |
|---|---|
| 条目数 | **52 条 title**（+ `platform-db` 6 个码 = 58 个键） |
| 含 CJK 的 title | **无** |
| 非 ASCII 的 title | **无** |
| 重复键 | **无** |
| `title == code` 本身 | **无** |

**完备性测试**（`platform-server/tests/i18n_titles.rs`，6 条全过）：

```
TITLES = 58 行；各 crate 的 code 合计 75 条（去重后 58 条）
```

## 6. 运行时验证（**真实 HTTP**，非静态）

起真实服务（`base_app` 普通角色，端口 8091），触发真实错误：

| 场景 | 状态 | 响应体 | 判定 |
|---|---|---|---|
| 未认证访问受保护接口 | 401 | `{"code":"UNAUTHENTICATED","title":"Unauthenticated",…}` | ✅ 零 CJK |
| 错误密码登录 | 401 | `{"code":"INVALID_CREDENTIALS","title":"Invalid Credentials",…}` | ✅ 零 CJK |
| 不存在的 schema 实体 | 404 | `{"code":"SCHEMA_ENTITY_NOT_FOUND","title":"Schema Entity Not Found",…}` | ✅ 零 CJK |

> 探针首版把**业务数据**（角色名「E2E 只读角色」）与 **SPA 的 index.html**
> 也算成「错误体含中文」→ 误报 3 条。修正口径后通过。
> **业务数据里的中文是允许的**（那是数据不是文案）。

## 7. 语言协商（D110 观测通道）—— 真实 HTTP 逐场景

| # | 输入 | `locale` | `source` | 判定 |
|---|---|---|---|---|
| ① | `?locale=en-US` + `Accept-Language: zh-CN`（**相反**） | `en-US` | **`user`** | ✅ **AC-S-090 步骤 8 的唯一有效判据** |
| ② | 只有 `Accept-Language: zh-CN` | `zh-CN` | `accept-language` | ✅ |
| ③ | `?locale=fr-FR`（**未注册**）+ 头 `zh-CN` | `zh-CN` | `accept-language` | ✅ 回退到请求头，**不是** `fr-FR` |
| ④ | `Accept-Language: *` | `zh-CN` | `default` | ✅ 平台默认，**不做任意匹配** |

`sources` 把语言/时区/币种**分开**记录（时区 = `tenant`，**永不** `accept-language` —— D114）。

## 8. `Money`（AC-S-096/097）

`platform-core` 的 money 测试 **19 条全过**，含：

- `addition_overflow_is_reported_not_wrapped`（`i64::MAX + 1` → `MONEY_OVERFLOW`，非 panic/回绕）
- `the_intermediate_product_uses_i128`（`convert(i64::MAX, 1_000_000)` 精确 == `i64::MAX`）
- `half_cases_round_to_even`（银行家舍入的 `.5` 边界）
- **`negative_amounts_round_symmetrically`** ← **负数舍入**
- `rounding_is_symmetric_for_a_range_of_rates`（**6 个汇率 × 8 个金额**的严格对称性）

> 负数这条**比 design 要求更严** —— design §5.4 只要求「归一化符号」，
> 实现加了「每个正数结果与对应负数结果严格互为相反数」的一般性判据。
> 这直接封死了 Rust `%` 向零截断导致负数偏向零侧的坑。

## 9. 前端 `Intl` 基准（BA 独立实测，Node v26.13.1）

| locale | 货币 | minorUnits | 实际输出 |
|---|---|---|---|
| `zh-CN` | CNY | 123456 | `¥1,234.56`（U+00A5） |
| `en-US` | CNY | 123456 | `CN¥1,234.56` |
| `ja-JP` | CNY | 123456 | `元` + **NBSP U+00A0** + `1,234.56` |
| `ja-JP` | JPY | 123456 | `￥123,456`（**U+FFE5 全角**） |
| `zh-CN` | JPY | 123456 | **`JP¥123,456`**（U+00A5，**带 JP 前缀**） |

`formatDate`（`2026-01-15T00:00:00Z`）：

| timeZone | 输出 |
|---|---|
| `Asia/Shanghai` | `2026-01-15 08:00:00` |
| `America/New_York` | **`2026-01-14 19:00:00`**（跨日） |
| `Europe/London` | `2026-01-15 00:00:00`（**1 月 = GMT = UTC**） |

**前端的断言与本表逐字一致**（显式写了 `\u00a5` / `\uffe5` / `\u00a0` 码点）。

---

## 结论（BA 视角）

**可以进入合并流程**，但**必须先过 QA Pre-merge**。

本记录覆盖的是「**实现是否做到了规格说的**」；
QA 的职责是「**规格是否被完整、正确地实现了**」—— 两者独立。

**BA 已知的开放项**（不构成阻塞）：

| # | 项 | 处置 |
|---|---|---|
| 1 | `platform-db` 的 6 个码在 `TITLES` 里是「死条目」 | 有意取舍（design §2.3.1 已写明），**QA 不要报成缺陷** |
| 2 | `packages/schema` 的 10 处中文文案未迁 | D117 / TD-015，**本轮不验收** |
| 3 | 金额 JSON 传输上限 2^53 | D118 / TD-016，**本轮不改契约** |
| 4 | `cargo fmt --all --check` 基线就红（8 个存量文件） | TD-017，**不是本轮引入** |
| 5 | `capabilities/**` 是生成物 | `cargo xtask capabilities` 生成，**不要手改** |
