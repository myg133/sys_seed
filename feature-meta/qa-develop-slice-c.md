# develop 切片 C（可观测性与工程工具）独立 QA

> 复查对象：`code/` @ `develop 164e32a`（合并 REQ-019 可观测性与 API 治理）
> 范围：`platform-observability/src`（2793 行）、`platform-core/src`（8166 行）、`xtask/src`（5791 行），合计约 16750 行
> 方法：静态走查 + 实测（cargo test / xtask --check）+ **变异攻击**（改坏守卫看是否变红）

## 结论

**限流修复（F15 / TD-077）成立，可以采信。** 三个怀疑点全部独立验证通过：

1. **分支能覆盖生产路径** —— `client_info` 只填 IP/UA（`crates/platform-server/src/client_info.rs:57-63`，全文 `with_tenant` 出现 0 次），限流挂在它内层，故 `ctx.tenant_id()` 在限流执行时确为 `None`，必然走第 ④ 步的令牌解析分支。
2. **真的验签且真的注入** —— `TokenTenantResolverImpl::resolve` 调 `IamService::verify_access_token`（`main.rs:1336`），后者走 `keys.verify` 做 JWK 验签 + `validate_exp/nbf/aud/iss`（`platform-iam/src/jwt.rs:171-199`），**不是**解 payload。注入点是 `main.rs:807` 传入 `sso_iam.clone()`，而 `sso_iam` 在 IAM 配置存在时被赋值（`main.rs:419`）。
3. **判据有区分力** —— 我把令牌解析分支改成恒 `None` 做变异，组 A **立刻变红**（`middleware_assembly.rs:595` 断言 429 实得 200），还原后复绿。

但范围外发现 **2 条 MAJOR**：`prune` 的注释承诺与实现矛盾（"0 用 60 兜底"实际是保留 2 秒），以及新端点 `/api/observability/rate-limit/status` **完全无认证**。

**整体判定：PASS（带 2 条 MAJOR 需处置）。** 限流修复本身与三个 xtask 守卫均经变异验证有效；MAJOR 缺陷不推翻切片 C 的主体结论，但上线前应确认。

## FINDING 清单

| # | 严重级 | 位置(file:line) | 问题 | 证据 |
|---|---|---|---|---|
| 1 | MAJOR | `crates/platform-observability/src/rate_limit/mod.rs:278-279` | `prune` 注释承诺「传 0 或负数用 **60 兜底（防误删全表）**」，实现是 `max_window_seconds.max(1) * 2` → 传 0 实际保留期 **2 秒**。注释与代码直接矛盾，"防误删"的承诺不成立 | 实测：实跑 `prune-rate-limit --max-window-seconds 0` 打印「保留期 0 秒 = 窗口 0 秒 × 2」（见下）。测试 `rate_limit_real_db.rs:827-835` 自认「30 秒的行**会**被删」 |
| 2 | MAJOR | `crates/platform-observability/src/http.rs:271-275`；`crates/platform-server/src/main.rs:818-821` | `/api/observability/rate-limit/status` **无任何认证层**。该 router 只 `.route(...)` + `.with_state(...)`，`main.rs` 直接 `nest` 上去，未套 `authenticate`。匿名可读全局限流配额 | 实测：无 `Authorization` 头 `GET /rate-limit/status` → **200** + 三行配额 `{"dimension":"tenant","requests":100,"windowSeconds":60}` |
| 3 | MINOR | `crates/platform-observability/src/deprecation.rs:13-14` vs `:47-48`、`:182-183` | 同一文件注释自相矛盾：文件头称判据用 `time` 的 **`Rfc7231` 格式化器反解**验证；正文却说「`time 0.3.55` 的 well_known 里**没有 `Rfc7231`**」，测试里也改用 `format_description` 手工描述 | 两处文字直接冲突；`:185` 实际用 `format_description::parse` |
| 4 | MINOR | `crates/xtask/src/main.rs:88-95` | prune 在 `tenants == 0` 时仍 `Ok(ExitCode::SUCCESS)`。RLS 导致「看不见」≠「没有数据」，cron/监控无法据退出码区分 → 清理可能静默失效、表继续无限增长 | 实测 `--max-window-seconds 0` 输出「0 个租户」且 `exit=0` |
| 5 | MINOR | `crates/platform-observability/src/http.rs:54-61` vs `BA/demands/REQ-019-observability-api/design-summary.md:399-408` | 设计文档 §10.1 承诺响应字段 `{dimension,key,limit,windowSeconds,used}`，实现只返回 `{dimension,requests,windowSeconds}`。**`used`（当前用量）缺失**，而"当前用量"正是端点存在理由 | 实测响应体无 `key`/`limit`/`used` |
| 6 | NIT | `crates/xtask/src/rustscan.rs:63-64` | `scan_traits` 用 `dedup_by(|l,r| l.name == r.name)` 只按 trait 名去重 → 不同文件中的同名 trait 会静默丢掉一条，索引漏项 | 当前平台无同名 trait（已核对），未触发；属潜在脆弱点 |

## 逐条详情

### FINDING 1（MAJOR）prune 的「60 兜底」承诺不成立

`crates/platform-observability/src/rate_limit/mod.rs:278-279`：

```rust
// ⚠️ 至少保留 2 倍窗口；若传入 0 或负数，用 60 兜底（防误删全表）。
let keep = max_window_seconds.max(1) * 2;
```

注释说「0 或负数用 60 兜底」，代码是 `.max(1) * 2` → 0 变成 keep=2 秒。

**实测证据**：

```
$ cargo run -q -p xtask -- observability prune-rate-limit \
    --database-url "postgres://base:base@127.0.0.1:5432/base_test" --max-window-seconds 0
限流清理完成：0 个租户，删除 0 行（保留期 0 秒 = 窗口 0 秒 × 2）
```

注意输出里「保留期 **0** 秒」——`main.rs:82` 打印的是 `keep * 2`（未走 prune 内部的 `.max(1)`），
即**打印值与实际删除保留期（2 秒）也不一致**，两层都有偏差。

**影响**：CLI 默认 60（`cli.rs:73`），正常路径不受影响。但运维显式传 0（意图可能是"清全部"或误传）
会得到「保留 2 秒」，而非注释承诺的 60 秒兜底 → 仍在窗口内的行被删 → 限流计数凭空变少
→ 实际放行量超过配额（正是 `cli.rs:68-69` 自己警告的危害）。

**旁证**：现有测试 `rate_limit_real_db.rs:827-835` 的注释已经把这行为写出来了
（"`max_window.max(1) * 2` → 保留期至少 2 秒 → 30 秒的行**会**被删"），
即实现者知道，但 `prune` 的文档注释没同步改 —— 属"注释与代码不一致"。

### FINDING 2（MAJOR）新端点无认证

- `crates/platform-observability/src/http.rs:271-275`：`router()` 只做了 `.route(...).with_state(...)`，
  **没有任何 `layer` 做认证**。
- `crates/platform-server/src/main.rs:818-821`：直接 `app.nest("/api/observability", ...)`，
  没有像 `/api/org`、`/api/rbac` 那样由子系统自带 `authenticate`。
- 对比：其余 8 个 `/api/*` nest 均由各自 crate 的 `authenticate` 中间件保护
  （`grep nest("/api` 见 main.rs:412/432/441/494/540/572/657/675）。

**实测证据**（临时探针，已删除）：无 `Authorization` 头请求该端点 → `200`，
响应体 `{"items":[{"dimension":"tenant","requests":100,"windowSeconds":60},...],"total":3}`。

**影响**：匿名攻击者可读到全局限流阈值（三个维度的 `requests` / `windowSeconds`），
据此精确卡在配额边缘发起请求，或据此判断哪些维度未启用（`requests: 0`）。
属信息暴露，不是凭据泄漏 —— 但设计文档 `design-summary.md:410-412` **只提到**
「这个端点本身也受限流保护」，**从未说明它应该是公开端点**，因此更像设计遗漏而非有意选择。

### FINDING 3（MINOR）deprecation.rs 注释自相矛盾

```
:13-14  判据用 `time` 的 `Rfc7231` 格式化器**反解**验证（ISO8601 与时间戳都解不开 → 红）。
:47-48  **实测**：`time 0.3.55` 的 `format_description::well_known` 里 **没有 `Rfc7231`**。
:182-183 ⚠️ `time 0.3.55` 没有 `Rfc7231` → 用 format_description 手工描述
```

文件头（读者最先看的地方）承诺的机制在正文被明确否定，实际测试用的是手工描述的 `format_description`。
这不影响功能（`Sunset` 格式化本身经实测正确：`2027-06-30` → `Wed, 30 Jun 2027 00:00:00 GMT`，
星期与月份映射已核对无误），但违反项目"注释与代码一致"的硬规矩。

### FINDING 4（MINOR）prune 退出码无法区分「RLS 看不见」

`main.rs:88-95`：`tenants == 0` 只打印警告，仍返回 `SUCCESS`。
`observability.rs:117-119` 的文档也承认「`tenants == 0` **不等于**『没有过期行』」。

**影响**：定时任务（cron/systemd timer）无法据退出码发现"用错连接角色导致一个租户都看不到"，
表会继续无限增长而无告警。这直接削弱了 FINDING 清单 #1 想防的"表无限增长"目标。
建议：`tenants == 0` 且库中确有数据时返回非 0，或至少输出机器可读标记。

### FINDING 5（MINOR）端点响应字段少于设计承诺

设计 `design-summary.md:399-408` 的示例含 `key` / `limit` / `used`；
实现 `http.rs:54-61` 只有 `dimension` / `requests` / `windowSeconds`。
其中 `used`（窗口内已用量）缺失意味着端点名"status"名不副实 —— 它只报配置，不报用量。

⚠️ 注意：AC-S-034 只关心分页契约字段（`limit`/`cursor`/`nextCursor`/`total`），
**不**要求 `used`，因此这条不构成 AC 违规，仅设计-实现偏差。

### FINDING 6（NIT）rustscan 按名去重的潜在漏项

`rustscan.rs:63-64` 排序后按 `name` 去重。若两个文件各有一个同名 `pub trait`，
后者会被静默删除，`extension-points.json` 少一条。
已核对当前 `platform-core/src/extension/` 无同名 trait，**未触发**。

---

## 已查确认无问题的项

### 限流修复（重点复核对象）

- ✅ **已查：生产装配顺序** —— `apply_all_middlewares_with`（`http.rs:476-507`）先挂 rate_limit（最内），
  最后挂 `client_info`（最外）→ 实际执行 `client_info → deprecation → trace_id → rate_limit → 业务`，
  与注释声明一致。租户确实只能由更内层的子系统 `authenticate` 提供 → 顶层必走令牌解析。
- ✅ **已查：`TokenTenantResolverImpl` 真验签** —— `main.rs:1336` 调 `verify_access_token`，
  其实现（`platform-iam/src/jwt.rs:171-199`）取 `kid` 找公钥、`Validation` 校验 exp/nbf/aud/iss、
  leeway 归零。**没有**只解 payload 的捷径。
- ✅ **已查：解析器真被注入** —— `main.rs:807` 传 `sso_iam.clone()`，`sso_iam` 在 `main.rs:419` 赋值。
  未配 IAM 时 `token_resolver` 为 `None` 并打 warn（`main.rs:1283-1288`）—— 此时确实不限流，
  但那时也没有可验证的令牌，属自洽降级。
- ✅ **已查：A/B/C 三组对照有区分力** —— 变异（解析分支→恒 `None`）后组 A 变红、组 B/C 仍绿，
  说明 A 锁住了"令牌→限流生效"、B 锁"无令牌→放行"、C 锁"无解析器→不生效"。三组各司其职。
- ✅ **已查：`an_existing_tenant_context_skips_the_token_resolver`** —— 用 panic 型解析器证明
  ctx 有租户时确实不重复验签（`middleware_assembly.rs:747-777`）。
- ✅ **已查：429 响应 trace_id 一致性** —— 实测 429 响应头 `X-Request-Id` 与响应体 `.trace_id`
  均为 `0123456789abcdef0123456789abcdef`（临时探针，已删除）。

### trace_id 入站校验（安全核心）

- ✅ **已查：校验顺序正确** —— `normalize_trace_id`（`trace/mod.rs:51-62`）先 ASCII hex 后长度，
  避免多字节 UTF-8 绕过。**变异验证**：改成只判长度后，
  CRLF 注入 / 多字节 UTF-8 / 控制字符 / 非 hex 四条判据**全部变红** → 判据有真实区分力。
- ✅ **已查：非法值不回显** —— `trace/middleware.rs:41-42` 在 resolve 阶段丢弃入站原值，
  `:99` 回写的是 `resolved`（已校验值）。响应头与响应体都不会出现入站非法值。
- ✅ **已查：`with_trace_id` 唯一调用点安全** —— 全仓只有 `trace/middleware.rs:50` 一处调用，
  传的是已校验的 `resolved`，不存在"未校验值经其它路径进入 ctx"。

### xtask 守卫有效性（变异验证）

- ✅ **已查：`check-deps` 有区分力** —— 给 `platform-observability/Cargo.toml` 加
  `sandbox = { path = "../../modules/sandbox" }` 后，`cargo run -p xtask -- check-deps` 退出码 **1**，
  并同时报出传递依赖链（`xtask → platform-observability → sandbox`）。还原后干净。
- ✅ **已查：`capabilities --check` 对**手改产物**有区分力** —— 手改 `capabilities/index.json`
  后退出码 **1**（`E_CAPABILITIES_OUTDATED`）。
- ✅ **已查：`capabilities --check` 对**源码变化**也有区分力**（更强的一条）—— 改
  `platform-core/src/extension/auditable.rs:44` 的 trait 方法签名后，退出码 **1**，
  同时报 `extension-points.json` 与 `index.json` 不一致。说明它比对的是重新生成的内容，不是时间戳。
- ✅ **已查：`openapi --check` 有区分力** —— 往 `openapi.json` 塞一条假路径后退出码 **1**，
  并逐行打印差异（`report_difference`）。
- ✅ **已查：`tenant-policies --check`** —— 基线通过（8 个模块 8 张表都有 RLS 策略），
  且**如实警告**当前连接角色 `base` 绕过 RLS（`tenant_policies.rs:67-74`），没有假装通过。

### 其他正面确认

- ✅ **已查：OpenAPI 未夸大覆盖** —— 产物只有 1 条路径，`description` 明写
  「⚠️ 当前只覆盖部分端点，见 TD-073」，与 `openapi.rs:9-15` 声明一致。
- ✅ **已查：限流唯一的每请求写表有清理路径** —— `platform_rate_limit` 是新增表里唯一
  「每请求一行」的（`grep INSERT INTO` 确认），清理命令实测可跑：删除了 2 行。
  设计文档 `design-summary.md:372-378` 的定期任务表已登记、迁移注释也指向该命令。
- ✅ **已查：`{tenant}` 参数绑定防注入** —— `TenantQuery::render`（`query.rs:252-283`）
  强制恰好一个 `{tenant}` 占位符并替换成 `$1` 绑定，租户 ID 不进 SQL 文本。
- ✅ **已查：配置读取是平台作用域** —— `platform-observability/manifest.json` 的 7 个
  `rateLimit.*` 均为 `"scope": "platform"`，因此 `_probe` 哨兵租户读配置**不会**跨租户泄漏。
- ✅ **已查：429 响应体不泄漏内部细节** —— `error.rs:93-96` 的 `RateLimitUnavailable` detail
  是固定文案，`reason` 只进日志；`params` 只有 `dimension`/`retryAfter` 两个键
  （`error.rs:104-124`），实测响应体确认。
- ✅ **已查：OTel fail open** —— `init_otel` 任何失败都 warn + 返回 `disabled`（`otel.rs:129-139`），
  `Drop` 里 shutdown 失败也只 warn（`otel.rs:96-104`），不 panic。
- ✅ **已查：弃用头精确匹配** —— `deprecation_by_path`（`deprecation.rs:141`）用 `*p == path`
  精确比对，不做前缀匹配，`/api/audit/events/extra` 不会继承弃用标记。
- ✅ **已查：`Sunset` 格式正确** —— 实测 `2027-06-30` → `Wed, 30 Jun 2027 00:00:00 GMT`，
  星期/月份映射与 Python `strftime` 交叉核对一致。

## 未验证项

1. **真实 OTLP collector 环境下的导出行为** —— 只确认了"构造失败不 panic"与
   `InMemorySpanExporter` 路径；未接真实 collector 验端到端上报。
2. **RLS 应用角色下的 prune 实际行为** —— 本机 `base` 是超级用户（`--check` 明确警告），
   没有非 `BYPASSRLS` 的应用角色可测，因此"应用角色跑 prune 会看到 0 个租户"这一说法
   **未实测**（只有代码与文档支持）。
3. **`tenant-policies` 对"库里有 tenant_id 列但 manifest 未声明"的表是否漏检** ——
   `check_scopes` 只校验 manifest 已声明的表，未做反向扫描（库里多出来的表不会被发现）。
   这是能力边界，未构造实例验证其后果。
4. **`/api/observability/rate-limit/status` 是否在其他鉴权模式（`auth_mode=sso`）下被网关保护** ——
   仅从代码确认应用层无认证；部署侧是否另有网关拦截无法从此仓库判定。

## 环境说明

本次复查期间，工作目录 `/home/myg133/Projects/system_seed/code` 存在**另一个 agent 的并发改动**：
`crates/platform-iam/tests/iam_real_db.rs` 被追加了一段「QA 临时实验」（`qa_concurrent_recovery_code_double_use`），
时间戳落在本次会话期间，但**非本会话所为**，且属切片 A/B 范围（IAM），不在本次复查范围内，故未触碰。
本次所有变异均在其后 `git diff --stat` 确认为空；切片 C 的 4 个目标文件
（`rate_limit/middleware.rs`、`trace/mod.rs`、`context.rs`、`deps.rs`）最终 `git status` 均为干净。
由 `Cargo.lock` 曾被变异污染（已于本次会话内 `git checkout -- Cargo.lock` 还原）。
