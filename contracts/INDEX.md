# 契约索引

> 契约正文进 `BA/contracts/`，消息只做通知与回执。
> 已 `ratified` 的 `vN.md` **不可修改** —— 破坏性变更升大版本，旧版保留一个 Sprint。

## 契约清单

| 契约 | owner | 当前版本 | status | consumers | 说明 |
|------|-------|----------|--------|-----------|------|
| [api-conventions](api-conventions/CURRENT) | myg133-ba-1 | v1 | ratified | REQ-002 ~ REQ-020 | 跨模块共享的接口约定：API 分组、路径形式、上下文请求头、权限点映射、越权码、错误体、分页、幂等、配置键、审计字段、命名规范 |
| [extension-points](extension-points/CURRENT) | myg133-ba-1 | **v2** | ratified | REQ-003 ~ REQ-020 | **业务模块与平台之间的接缝**：5 个扩展点 trait 签名、编译期注册、契约检查 C-1~C-5、manifest 结构。含 **dyn 兼容性限制**（RPITIT 形态，不可用于 trait object） |

### 版本沿革

| 契约 | 版本 | 状态 | 说明 |
|------|------|------|------|
| extension-points | v1 | **deprecated**（保留一个 Sprint） | 5 个 trait + C-1~C-4。**已知缺口**：C-2 只验权限点声明完整性，不验服务端是否真的执行鉴权 |
| extension-points | **v2** | ratified（当前） | 新增 **C-5：已登记权限点必须有鉴权调用点**（关闭 v1 缺口）；manifest 新增可选 `permissions[].condition`。C-1~C-4 与 5 个 trait 签名**不变** |

> ⚠️ **v2 对消费方是破坏性变更**：新增的 C-5 是**失败级**检查。
> `modules/sandbox` 在 v1 下 C-2 全绿但从不鉴权，v2 下会失败 —— 这是预期效果（REQ-009 内已修）。
> 消费方唯一要做的事：**确保每个已登记权限点都有鉴权调用点**。

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
