-- 通知系统的 5 个配置项默认值回滚（REQ-015，design §6 / D304 / D313）。
--
-- 只删**本迁移**登记的那 5 个键 —— 不碰任何其它配置行。
--
-- ⚠️ `platform_config` 的直接 DML 已被 REVOKE（REQ-010 D56），
-- 但**迁移跑在表 owner 上** → 可以直接 DELETE（回滚是 owner 权限的活）。
--
-- ⚠️ 回滚**不**自增 epoch（与 REQ-016 / REQ-022 的 down 同款）——
-- 回滚是运维动作，之后的读取会在 TTL（60s）内可能看到旧值；
-- 需要立刻生效就再调一次 `platform_config_upsert`（或重跑 up）。

DELETE FROM platform_config
 WHERE key IN (
       'notify.maxAttempts',
       'notify.retryBaseSeconds',
       'notify.retryMultiplier',
       'notify.retryMaxSeconds',
       'notify.deliveringTimeoutSeconds'
 );
