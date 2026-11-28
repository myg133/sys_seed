# 契约索引

> 契约正文进 `BA/contracts/`，消息只做通知与回执。
> 已 `ratified` 的 `vN.md` **不可修改** —— 破坏性变更升大版本，旧版保留一个 Sprint。

## 契约清单

| 契约 | owner | 当前版本 | status | consumers | 说明 |
|------|-------|----------|--------|-----------|------|
| [api-conventions](api-conventions/CURRENT) | myg133-ba-1 | **v3** | ratified | REQ-002 ~ REQ-022 | 跨模块共享的接口约定：API 分组、路径形式、上下文请求头、权限点映射、越权码、错误体、分页、幂等、配置键、审计字段、命名规范、OAuth 协议端点的错误体例外 |
| [extension-points](extension-points/CURRENT) | myg133-ba-1 | **v2** | ratified | REQ-003 ~ REQ-020 | **业务模块与平台之间的接缝**：5 个扩展点 trait 签名、编译期注册、契约检查 C-1~C-5、manifest 结构。含 **dyn 兼容性限制**（RPITIT 形态，不可用于 trait object） |
| [external-module-integration](external-module-integration/v1.md) | myg133-ba-1 | **v1** | **draft** | 首个：AI-Bridge（Go） | **进程边界**：外部系统以独立进程接入时的责任边界、签名上下文、权限/审计/配置契约、**管理面描述（admin）**、接入流程、分发与准入。配套：[接入流程](external-module-integration/onboarding.md)、[分发](external-module-integration/distribution.md)、[检查表](external-module-integration/checklist.md)、[prompts](external-module-integration/prompts/README.md) |

### 版本沿革

| 契约 | 版本 | 状态 | 说明 |
|------|------|------|------|
| api-conventions | v1 | **deprecated**（保留一个 Sprint） | 首版 13 条约定。C9 / C10 有四处未定死（`source` 缺 `default`、作用域语义含糊、值类型校验未定、灰度确定性未定） |
| api-conventions | v2 | **deprecated**（保留一个 Sprint） | C9 新增 `source: default` / 作用域约束 / 值类型校验 / `jsonb` 存储；C10 新增灰度确定性、无身份 fail closed、未声明 flag 不 404。**C1~C8、C11~C13 一字未改** |
| api-conventions | **v3** | ratified（当前） | **C1** 增加 `/api/oauth` 前缀 + C1.1 授权服务器元数据端点；**C14**（新增）OAuth 协议端点的错误体**不用** Problem Details（用 RFC 的 `{"error": …}`）。**C2~C13 一字未改**；对既有消费方**零影响**（纯新增） |
| extension-points | v1 | **deprecated**（保留一个 Sprint） | 5 个 trait + C-1~C-4。**已知缺口**：C-2 只验权限点声明完整性，不验服务端是否真的执行鉴权 |
| extension-points | **v2** | ratified（当前） | 新增 **C-5：已登记权限点必须有鉴权调用点**（关闭 v1 缺口）；manifest 新增可选 `permissions[].condition`。C-1~C-4 与 5 个 trait 签名**不变** |
| external-module-integration | **v1** | **draft**（评审中） | 首版。定义平台与外部模块的**进程边界**：签名上下文、路由级权限映射、outbox 审计、**能力可选（manifest/config 必选，其余按需）、配置托管（平台唯一持久化、四层合并、TTL 缓存）、平台→模块事件通道**、**契约注册（Pull/Push/导入）**、**公网前提下的两种可达性与降级**、**配置查询接口 + 平台侧可配 TTL（默认 120s）缓存语义**、**管理面描述渲染（app/profile 双页面、对象平铺）**、契约分发与准入。含接入流程、分发机制、准入检查表、4 个接入 prompt、JSON Schema、两份 OpenAPI 与示例校验。不改 extension-points 既有字段语义 |

> ⚠️ **extension-points v2 对消费方是破坏性变更**：新增的 C-5 是**失败级**检查。
> `modules/sandbox` 在 v1 下 C-2 全绿但从不鉴权，v2 下会失败 —— 这是预期效果（REQ-009 内已修）。
> 消费方唯一要做的事：**确保每个已登记权限点都有鉴权调用点**。
>
> ⚠️ **api-conventions v2 的 `source` 新增 `default` 取值**。若客户端对 `source` 做了**穷举**
> （TS 的 `switch` + `never` 兜底、Rust 的 `match` 无 `_`），会编译失败。
> 消费方要做的事：把 `source` 当**开放枚举**处理，未知取值**原样透传或降级，不得报错**。
>
> ✅ **api-conventions v3 对既有消费方零影响**（纯新增）：
> C1 只是加了一行前缀（`/api/oauth`），C14 只约束 OAuth 协议端点。
> 唯一要留意的是：如果你**自己**实现了 OAuth 协议端点，
> 错误体要用 RFC 的 `{"error": "invalid_grant"}`，**不是** Problem Details。

## 消费方 pin 要求

每个 feature worktree 在自己的 `feature-meta/manifest.json` 中声明所 pin 的契约版本：

```json
{
  "contracts": {
    "api-conventions": "v1"
  }
}
```

合并前 `aw.py contract check` 会校验：pin 的版本存在、消费方齐全、无未回执的变更。

## 新增契约的流程

1. 在 `BA/contracts/<name>/` 建目录，写 `v1.md`（status: draft）
2. 在本文件登记
3. BA 评审 → 改 status 为 `ratified`，写 `CURRENT` 指针
4. 通知消费方 → 消费方在自己的 manifest 中 pin 版本
5. 变更时：升版本号，旧版标 `deprecated` 并写兼容窗口
