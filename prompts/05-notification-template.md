# 任务：新增通知模板与投递渠道

## 目标

让业务模块能发通知：定义模板、接一个投递通道。
**通道与消息模型由平台提供**，业务侧只做「把业务事件翻译成消息」。

## 输入

- `<模块名>`：业务模块名
- `<模板 key>`：通知模板标识（如 `invoice-approved`）
- `<通道>`：投递通道（如 `email` / `sms` / `webhook`）
- `<收件人来源>`：收件人从哪来（用户 / 角色 / 固定地址）
- `<模板变量>`：模板里可用的变量（标题、正文里的占位字段）
- `<触发时机>`：什么业务事件触发这条通知

## 步骤

1. **先读** `AGENTS.md` 与 `capabilities/index.json`：
   - `already_provided.notification_sender` —— 平台**已经**提供
     `NotificationSender` trait、`NotificationMessage`（标题 / 正文 / 收件人 /
     模板变量）、`DeliveryResult`（成功 / 失败 / 通道消息 ID）；
   - `not_yet_provided.notification_dispatch` —— 通道的真实投递与模板渲染在
     REQ-015，本期只做「业务侧接缝」。
   **不要自己写消息队列客户端、模板引擎或投递 SDK。**
2. **实现发送器**：在 `modules/<模块名>/backend/src/` 下实现
   `platform_core::extension::notification::NotificationSender`：
   - `channel()` 返回 `<通道>`，与通道标识对应；
   - `send(&self, ctx: &RequestContext, msg: &NotificationMessage)`
     返回 `DeliveryResult`。失败要返回 `DeliveryResult::failed(原因)`，
     不要把失败吞掉。
3. **组装消息**：用 `NotificationMessage::new(标题, 正文)`
   + `.with_recipient(...)` + `.with_payload(键, 值)` 组装消息。
   **模板变量的值来自业务数据，租户相关的数据只能从 `ctx` 取。**
4. **声明权限点**：发通知 / 管模板是独立权限点
   （`<模块名>:notify:send` 等），在 `manifest.json` 登记 +
   `backend/src/permission.rs` 加同名常量（C-2 会核对）。
5. **写契约测试**：在 `tests/contracts.rs` 里补一条「发送失败时 `DeliveryResult`
   标记为未投递」的用例；C-3 仍要求两个租户的同名数据。
6. **更新能力索引与文档**：`cargo xtask capabilities`，
   更新 `modules/<模块名>/AGENTS.md`。
7. **自验**：
   ```bash
   cargo test -p <模块名> -- --test-threads=2
   cargo xtask check-modules
   cargo xtask capabilities --check
   ```

## 约束

- 不修改 `crates/platform-core/` 与 `crates/platform-contracts/`；
  `NotificationSender` 的签名是 `extension-points v1` 冻结的。
- 不引第三方消息队列 / 邮件库（本期零新增第三方依赖是硬基线）。
- 通知正文里的用户可见中文属于**面向用户文案**，但当前后端约定是
  「只返回错误码 + 参数」——通知正文的模板化渲染随 REQ-015 落地，
  现在把正文当作模板 key 或参数传入，不要在后端拼最终中文长句。
- 收件人列表不能来自客户端传入的任意地址（避免变成邮件炸弹）。

## 验收

- [ ] 实现了 `NotificationSender`，`channel()` 与 `<通道>` 一致
- [ ] 投递失败时返回 `DeliveryResult::failed(...)`，没有静默成功
- [ ] 权限点已在 manifest 声明，C-2 无告警
- [ ] `cargo test -p <模块名> -- --test-threads=2` 全绿
- [ ] `modules/<模块名>/AGENTS.md` 的「用到的平台扩展点」含 `NotificationSender`
