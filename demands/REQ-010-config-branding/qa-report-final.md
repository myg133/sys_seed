# 第三轮聚焦复核报告 —— REQ-010

- **需求**: REQ-010「配置、品牌与 Feature Flag」
- **被验提交**: `0d0a1ee`（上一轮 `a4af6ee`）
- **前两轮归档**: [`qa-report.md`](./qa-report.md)（首轮 FAIL）/ [`qa-report-recheck.md`](./qa-report-recheck.md)（复核 PASS-WITH-RISK）
- **范围**: 只验 BA 本轮修的 3 条 + 回归；15 条 AC 的其余证据沿用前两轮
- **验证者**: QA Agent（独立复跑，**不采信 Dev/BA 自报数字**）
- **日期**: 2026-11-19
- **结论**: **PASS-WITH-RISK** —— 三条主项**全部真修好**（快照变异在本轮实测被抓到），回归干净；
  仅剩 3 处 **LOW 级注释/文档漂移**，不影响产品行为与断言强度。

> 变异全部逐字节还原：`service.rs` sha256 `78d330d7…0842` 与还原前一致，`git status` 中
> `crates/` 为空，无 `QA-MUTATION` 残留；测试库无变异写入残留（`platform_tenant_config`
> 的 `conflictStrategy` 行数 = 0，空间级行仍是夹具值 `remote_wins`）。
> 工作区仅剩 `feature-meta/qa-report.md` / `qa-report-recheck.md` 两处**进入本轮前就存在**的未提交改动，我未触碰。

---

## 0. 逐条结论

| # | 问题 | 结论 | 一句话证据 |
|---|---|---|---|
| **1** | 快照断言：取点晚、缺 `updated_at` | **PASS** | 取点在 `:272`、首次越级写 `:275`；SQL 含 `max(updated_at)`；变异「越级写落库但仍返回 400」→ **红在 `:359` 快照断言**，消息显示 value 未变、只有 `updated_at` 变 |
| **2** | `NOISY_TARGETS` 空转 | **PASS（有 2 处注释瑕疵，LOW）** | 只剩 `sqlx`，且它确有噪音（`sqlx-core` 依赖 `tracing`，默认 `statements_level=Debug`，target `sqlx::query`）；`tower_http` 已清；但注释对 `hyper_util` 的说法与事实相反、模块文档 `:19` 仍是旧列表 |
| **3** | 测试对 `RUST_LOG` 敏感 | **PASS（1 处注释漂移，LOW）** | 两个函数分工清楚、生产仍读 `RUST_LOG`、测试全用 `log_filter_ignoring_env`；`RUST_LOG=base=info` 与不带 `RUST_LOG` 均 **476/0**（另 70 ignored），带环境 + `--include-ignored` 全量 **540/0** |
| **回归** | —— | **PASS** | 集成 **540/0**（28 suite）、clippy 0、fmt 干净、`bun run test` Rust 476/0 + 前端 **28/0**、E2E **2 passed** |

---

## 1. 快照断言（本轮最关键）

### 1.1 取点：确认在步骤 1 之前

`crates/platform-config/tests/config_real_db.rs`：

| 行号 | 内容 |
|---|---|
| `:272` | `let before_snapshot = conflict_strategy_snapshot(&fx.admin_pool).await;` |
| `:275` | 第一次 `write_tenant(... "conflictStrategy" ...)`（步骤 1，期望 400） |
| `:358` | `let after = conflict_strategy_snapshot(...)` |
| `:359` | `assert_eq!(after, before_snapshot, ...)` |

`272 < 275`，**写前快照确实取在任何越级写之前**（上一轮是取在步骤 1/2 之后）。✅

### 1.2 SQL：确实选了 `updated_at`，注释与代码一致

`conflict_strategy_snapshot`（`:374`）的 `UNION ALL` 两个分支都选了
`max(updated_at)::text`；返回类型是 **6 元组**（`String, String, Option<String>, i64, Option<String>, Option<String>`），
与 SQL 的 6 列一一对应。`:353-357` 的注释现在写的也是「`max(updated_at)` 抓同值覆盖」——**注释不再超前于代码**。✅

> 仅有一处小瑕疵（INFO，不判缺陷）：`:365` 的函数头摘要仍写
> `(表, tenant, space, count, max(value))`（5 项），实际是 6 项，漏了 `updated_at`。
> 正文解释是对的，属摘要未同步。

### 1.3 变异 A（我自己做的）：同值覆盖 + 仍返回 400 → **红在快照断言**

变异内容：在 `service.rs` 第 5 步作用域校验里，仅对
`scope == Space && key == "conflictStrategy"`，**先执行 `store.write_scoped` 落库，再返回原来的 `ScopeNotAllowed`**
（即「报 400 但已写库」，且写的值与夹具相同 `remote_wins`）。

```text
test scope_escalation_is_rejected_and_leaves_no_rows ... FAILED
panicked at crates/platform-config/tests/config_real_db.rs:359:5:
assertion `left == right` failed: 越级写改动了既有行（快照不一致）：
before=[("space","tenant-a",Some("space-sales-1"),1,Some("\"remote_wins\""),Some("2026-11-18 18:56:08.08249+00"))]
after =[("space","tenant-a",Some("space-sales-1"),1,Some("\"remote_wins\""),Some("2026-11-18 18:56:08.212981+00"))]
test result: FAILED. 0 passed; 1 failed
```

判据逐条对上：

1. **红在 `:359`（快照 `assert_eq!`），不是步骤 1 的 `expect_err`** —— 步骤 1/2 的 `expect_err` 都通过了（仍返回 400），
   说明红点确实是快照断言，符合本轮验收动作的字面要求。
2. **失败信息显示 `count` 1=1、`value` 都是 `"remote_wins"`（未变）、只有 `updated_at` 从 `.08249` 变成 `.212981`**
   —— 正是「同值覆盖只改时间戳」这个上轮漏检的场景。
3. 库侧独立确认（`podman exec base-postgres psql`）：`tenant-a|space-sales-1|conflictStrategy|"remote_wins"|…212981`，
   即泄漏真实发生、且旧口径（count+value）确实看不见。

顺带补测第二种形态（上轮另一条漏检路径）：把变异条件改成 `scope == Tenant`（租户级新增行），
同样**红在 `:359`**，`after` 多出一条 `("tenant","tenant-a",None,1,Some("\"remote_wins\""),Some(...))`
—— 说明「取点提前」也堵住了上一轮 `before_snapshot` 晚取导致的漏检。

### 1.4 还原

- `cp` 回原文件 + `touch`（避免上轮「cp -a 保留 mtime → 复用变异二进制」的坑）；
- `sha256sum -c` 通过（`78d330d7…0842`）；`git status crates/` 为空；`grep QA-MUTATION` = 0；
- 复原后重跑该用例：**1 passed**。

---

## 2. `NOISY_TARGETS`

`crates/platform-core/src/log.rs:54`：`pub const NOISY_TARGETS: [&str; 1] = ["sqlx"];`

### 2.1 留下的 `sqlx`：在依赖树里，且真的会产生噪音 ✅

| 判据 | 证据 |
|---|---|
| 在 `Cargo.lock` / 依赖树 | `cargo tree -i sqlx-core` 有输出；`sqlx-core` 依赖 `tracing v0.1.44`（`Cargo.toml` 中 **非 optional**，`features=["log"]`） |
| 会产生噪音 | `sqlx-core/src/connection.rs:197` 默认 `statements_level: LevelFilter::Debug`；`logger.rs` 以 target `sqlx::query` 在 Debug 打 `query=` 与耗时 |
| 降噪方向正确 | `EnvFilter` 的 `sqlx=info` 按 target 前缀命中 `sqlx::query`，把逐条 SQL 压到 info 以下 |

### 2.2 `tower_http` 已清干净 ✅

`Cargo.lock` 中 `name = "tower-http"` 计数 **0**，全仓 `Cargo.toml` 无引用 —— 注释「根本不在依赖树里」正确。

### 2.3 `hyper`：注释正确 ✅

`cargo tree -e features -i hyper` 中**没有** `hyper feature "tracing"`（唯一命中是依赖者的 `axum feature "tracing"`）；
`Cargo.lock` 里 hyper 的 12 个依赖不含 `tracing` —— 注释「hyper 未启用 tracing feature」与事实一致。

### 2.4 **LOW**：注释对 `hyper_util` 的判断是错的

`log.rs:52` 写「`hyper` / `hyper_util` **若哪天开了** tracing feature，就该在这里加一条」。
但 `hyper_util` **今天就已经开着了**：

- 根 `Cargo.toml:103`：`hyper-util = { version = "0.1", features = ["client-legacy", "http1", "tokio"] }`；
- `hyper-util` 的 `client` feature 定义含 `dep:tracing`，`client-legacy` 又依赖 `client`；
- `cargo tree -p hyper-util --edges normal --depth 1` 显示 **`tracing v0.1.44` 是它的直接依赖**；
- 源码里确有常规出站日志：`client/legacy/connect/http.rs:786` `debug!("connecting to {}")`、`pool.rs:267` `debug!("reuse idle connection for {:?}")` 等。

影响：`BASE_LOG_LEVEL=debug` 时，`platform-org` 的出站 HTTP（LDAP/通知）会打 debug 行，而 `NOISY_TARGETS` 压不住它。
**这不构成回归**（上轮清单里也没有它），但注释给的「它没开 tracing」这个理由不成立 ——
排除它可以作为取舍，理由应改成「出站路径低频，暂不压」，而不是「feature 没开」。
量级未实测（见 §5 存疑），故判 LOW。

### 2.5 **LOW**：模块文档仍写着旧的三个库

`log.rs:19`：

```text
{level},tower_http=info,hyper=info,sqlx=info
```

而 `NOISY_TARGETS` 已是 `["sqlx"]`。这与本轮修的原始缺陷同类（注释与代码不一致），
读者会以为 `tower_http` / `hyper` 仍被降级。功能无影响，纯文档漂移。

---

## 3. 测试对 `RUST_LOG` 的敏感性

### 3.1 两个函数分工清楚，生产仍尊重 `RUST_LOG` ✅

- `log_filter(level)`（`:59-63`）：`EnvFilter::try_from_default_env()` 成功就用它，**`RUST_LOG` 完全接管**；
  失败（未设/非法）才走默认形态。
- `log_filter_ignoring_env(level)`（`:74-78`）：完全不读环境，只构造「全局默认 + `NOISY_TARGETS` 降级」。
- 两者文档都写明用途（生产用前者、测试用后者），无歧义。

### 3.2 没有测试误用生产入口 ✅

全仓 `grep "log_filter("` 只有两处：定义（`log.rs:59`）和生产启动（`platform-server/src/main.rs:231` 的 `init_tracing`）。
测试侧（`platform-core` 单测、`platform-server` 单测、`config_real_db` 集成测）**全部**改用 `log_filter_ignoring_env`。
`crates/platform-core/src/bin/platform-core.rs:35` 另有一份小 bin 自己的 `init_tracing`（读 `RUST_LOG`），但它**没有任何测试**，
不会被环境劫持，且不是 `platform-server` 的生产路径。

### 3.3 两种环境实跑（独立复跑，非采信 Dev）

| 场景 | 命令 | 结果 |
|---|---|---|
| 带环境 | `RUST_LOG=base=info cargo test --workspace -- --test-threads=2` | **476 passed / 0 failed / 70 ignored**（39 个 suite），EXIT=0 |
| 不带环境 | `env -u RUST_LOG cargo test --workspace -- --test-threads=2` | **476 passed / 0 failed / 70 ignored**（39 suite），EXIT=0 |
| 带环境 + 覆盖 ignored | `RUST_LOG=base=info ./scripts/test-integration.sh` | **540 passed / 0 failed / 0 ignored**（28 suite），EXIT=0 |

三次通过数逐 suite 一致，上一轮「`RUST_LOG=base=info` 让平台侧 5 条红」的现象**不再出现**。
说明：plain `cargo test` 默认会跳过 70 条 `#[ignore]` 的真实库用例（rbac/iam/org/config 的 `*_real_db`），
所以我又用集成脚本（`--include-ignored`）在 `RUST_LOG=base=info` 下把 540 条**全部**跑了一遍；
其中 `config_real_db` 里唯一被 ignore 的是子进程辅助用例，`unknown_flags_warn_once…`（上轮被判红的日志断言）**确实在 476 条里跑到了**。

### 3.4 **LOW**：`main.rs:769` 的注释与本轮改动不一致

```rust
/// ⚠️ 这里调用的是 `init_tracing` 用的**同一个** `log_filter`（`platform_core::log_filter`），
fn log_filter_captures_platform_config_warn() {
    let out = capture_logs(log_filter_ignoring_env("info"), || {   // ← :773 实际调用
```

代码是对的（用不受环境影响的入口），但注释仍说「同一个 `log_filter`」——注释没跟着改。
与 §2.5 同类：**注释与代码漂移**。不影响测试行为，判 LOW。

---

## 4. 回归（全部我自己跑）

| 项 | 命令 | 结果 |
|---|---|---|
| 集成 | `scripts/test-integration.sh`（1 轮，未设 `RUST_LOG`） | **540 passed / 0 failed / 0 ignored**（28 suite）；容器数比对 `2 → 2`（未新建） |
| 集成（第 2 轮） | `RUST_LOG=base=info ./scripts/test-integration.sh` | **540 passed / 0 failed / 0 ignored**（28 suite）；容器数 `2 → 2` |
| clippy | `cargo clippy --workspace --all-targets -- -D warnings` | **0 warning / 0 error**，EXIT=0 |
| fmt | `cargo fmt --all --check` | 干净，EXIT=0 |
| Rust + 前端 | `bun run test`（带 `DATABASE_URL_TEST`） | Rust **476 passed / 0 failed**；前端 **28 passed / 0 failed**（9 个 test file） |
| E2E | `bun run test:e2e` | **2 passed**（8.1s），含 `shell 从品牌接口读取标题并渲染（同源）` |

与上一轮基线一致（集成 540、Rust 476、前端 28、E2E 2）——**未发现回归**。

> 备注：E2E 的 `playwright.config.ts` 设了 `reuseExistingServer: true`；本轮开始前 8080 端口空闲，跑的是新构建的服务，不存在复用陈旧进程的问题。

---

## 5. 未覆盖 / 存疑

| 项 | 说明 |
|---|---|
| `hyper_util` 出站日志的实际量级 | 源码级确认「tracing 已启用、连接/复用会打 debug」，但本轮**未触发** LDAP/通知出站路径，无法给出「每请求几行」的实测值 → **存疑** |
| 生产日志管道（Loki/ELK） | 只验了过滤器与 stdout，未在真实日志聚合上复验 |
| `RUST_LOG` 非法值的行为 | 代码路径是 `try_from_default_env` 失败即回落默认形态，未单独构造非法 `RUST_LOG` 实测（属既有行为，非本轮三问） |
| 其余 AC 的端到端证据 | 沿用前两轮，本轮未重跑（范围所限） |

---

## 6. 给 BA 的一句话

**三条都真修好了**：快照变异（同值覆盖 + 新增行两种）都被 `:359` 的快照断言抓住，且失败信息能直接看出「value 没变、只有 `updated_at` 变」；
`NOISY_TARGETS` 留下的 `sqlx` 名副其实；`RUST_LOG=base=info` 与不设 `RUST_LOG` 都是 476/0，回归 540/540。
剩下 3 处 **LOW 级注释/文档漂移**建议顺手清掉：`log.rs:19` 的旧三库形态、`log.rs:52` 关于 `hyper_util` 的「若哪天开了」、
`main.rs:769` 的「同一个 `log_filter`」；另 `config_real_db.rs:365` 的函数头摘要漏了 `updated_at`（INFO）。
这些不影响功能与断言强度，**不阻塞验收**。
