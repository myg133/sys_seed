# 需求：可观测性与 API 治理

- **需求编号**: REQ-019
- **父需求**: [REQ-001 通用项目底座](../REQ-001-platform-foundation/demand.md)
- **优先级**: P1
- **状态**: 已就绪
- **所属迭代**: Sprint 6
- **更新时间**: 2026-11-22

## 需求来源披露（D122）

⚠️ **BA 补充**，不是用户原始需求。

用户 2026-11-19 裁定「出了工作流，其他的继续做」→ 本需求继续做（D121）。

用户 2026-11-22 对本需求的三个技术口径裁定（见 D337~D339）：
1. 限流 = **PG 滑动窗口**（零新依赖，多实例准确）
2. OpenAPI = **引入 `utoipa`**（第三次依赖破例，净增实测 2 包）
3. OpenTelemetry = **真接**（第四次依赖破例，净增实测 18 包）

## 需求描述

让问题可定位、接口可治理。

底座的 19 个子需求已全部完成，但「出问题能不能查」和「接口能不能管」是两件**独立于功能**的事 ——
功能全绿不代表线上出问题时能定位。本需求补齐这两块。

## 依赖

- REQ-002（已完成）

## 现状基线（**2026-11-22 实测**，不是推测）

| # | 能力 | 现状 | 缺什么 |
|---|---|---|---|
| 1 | **`trace_id` 贯穿** | ⚠️ **部分** | `RequestContext` 有 `trace_id`（`crates/platform-core/src/context.rs:25`），响应体带（`main.rs:1737`）；但 `init_tracing`（`main.rs:233`）**没有把 trace_id 注入日志** → 日志里串不起来 |
| 2 | 结构化日志 | ⚠️ **部分** | `tracing_subscriber::fmt()` 已用；但缺 trace_id 关联字段 |
| 3 | RFC 9457 错误格式 | ✅ **已有且扎实** | `ProblemDetails`（`platform-core/src/problem.rs:89`）字段完整（`type`/`title`/`status`/`detail`/`instance`/`code`/`trace_id`/`params`），**22 个 crate 在用** |
| 4 | 健康检查 / 就绪探针 | ✅ **已有** | `/health/live` + `/health/ready`（`platform-server/src/health.rs`，`lib.rs:6` 有说明） |
| 5 | **三级限流**（租户/用户/接口） | ❌ **完全没有** | 只有 IAM 登录的**单点**防护（`platform-iam/src/service.rs:837` `check_ip_rate_limit`，且**无租户维度**）。没有通用中间件、**没有 429 响应** |
| 6 | 分页 / 排序 / 过滤规范 | ⚠️ **不统一** | `platform-audit` 有 `next_cursor`（`query.rs:368`）；其他 crate 各自实现（app 12 处 / iam 23 处 / notify 6 处 / storage 4 处），**rbac 与 org 完全没有** |
| 7 | **OpenAPI 自动生成** | ❌ **完全没有** | 树里无 `utoipa`、无任何 OpenAPI 产物 |
| 8 | **弃用策略**（Deprecation / Sunset 头） | ❌ **完全没有** | `grep -rn 'Deprecation\|Sunset' crates/platform-server/src/` 零命中 |
| 9 | **OpenTelemetry** | ❌ **完全没有** | 树里无 `opentelemetry` 系列 |

**结论**：9 项里 **2 项已完成**（RFC 9457 / 健康检查）、**4 项部分**、**3 项完全没有**。

## 功能要求

### 一、可观测性

1. **`trace_id` 全链路贯穿**：日志、错误响应体、审计事件携带**同一个** trace_id；
   从入站请求头 `X-Request-Id`（有则沿用）或自动生成，并**回写响应头**。
2. **OpenTelemetry**：接入 OTLP（HTTP/protobuf）导出；span 覆盖 HTTP 请求 →
   DB 查询 → 外部调用三层。
3. 健康检查与就绪探针：**已有**，本需求只补「就绪探针覆盖新依赖（限流存储）」这一项。

### 二、API 治理

4. **三级限流**：租户 / 用户 / 接口三个维度**各自独立**计数，
   超限返回 **429 + `Retry-After`**（秒数），且**不影响其他维度**。
5. **分页/排序/过滤规范**：统一契约（`limit` / `cursor` / `nextCursor` / `total` 的语义与命名），
   并让现有 crate 的响应**与之一致**（跨栈断言写「包含」不写「相等」，D194）。
6. **OpenAPI 自动生成**：从路由与 DTO 生成 `openapi.json`，配 `--check` 守卫防漂移。
7. **弃用策略**：被弃用的端点返回 `Deprecation` 与 `Sunset` 头（RFC 8594 / RFC 9745）。

## 不在范围内

- 业务系统的自定义指标（业务模块自行接入）
- 日志聚合后端（Loki / ELK）的部署 —— 那是部署侧的事，本需求只管**产出结构化日志**
- 链路后端（Jaeger / Tempo）的部署 —— 本需求只管**导出 OTLP**，不负责收集端

## 已知限制（明码记账，规格前就写清）

| 限制 | 为什么 | 记在哪 |
|---|---|---|
| 无 OTel collector → **无法端到端验证真导出** | 本机只有 postgres/redis | **TD-069** |
| 限流用 PG 滑动窗口 → **每次请求一次写+查** | 零依赖约束（无 Redis） | 见 D337 的取舍说明 |

## 关联验收标准

父需求 AC：**AC-027**（限流三级生效）、**AC-028**（trace_id 全链路贯穿）

本需求自有 AC：**AC-S-029 ~ AC-S-036**（8 条，见 [acceptance.md](acceptance.md)）

## 完成定义

- trace_id 在日志 / 响应体 / 审计事件里**是同一个值**（可断言）
- 三级限流**各自独立**生效，返回 429 + `Retry-After`
- `cargo xtask openapi --check` 退出码 0（产物与代码一致）
- 被弃用端点返回 `Deprecation` + `Sunset` 头
