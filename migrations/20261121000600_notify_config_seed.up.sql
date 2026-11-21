-- 通知系统的 5 个配置项默认值（REQ-015，design §6 / D304 / D313）。
--
-- # 登记要**两处**都有（漏 INSERT → 平台起不来 / 配置读不到）
--
-- 1. `crates/platform-notify/manifest.json` 的 `config` 段 —— 键的**登记**
--    （注册表在**启动时**扫描 manifest；没登记 → 写不进去）；
-- 2. 本迁移 —— 值的**落库**（平台级默认值写在 `platform_config`）。
--
-- # ⚠️ 键名是 **lowerCamelCase**（契约 C9）
--
-- `ConfigKey::parse` 要求每一段「**小写字母开头 + 只含字母数字**」
-- （`crates/platform-core/src/config.rs:213` 的 `is_lower_camel_case`）→
-- **带 `_` 的键名直接让服务起不来**。
--
-- # ⚠️ 5 个键都是 **`scope: "platform"`**（D304 / REQ-016 的 S-01 教训）
--
-- manifest 里 5 个键必须写 `"scope": "platform"` —— `ConfigSpec.scope` 的 serde
-- 默认值是 **`Tenant`** → 漏写就落到租户作用域 → 平台级读写端点直接拒
-- （`platform-config/src/service.rs` 的 `read_platform_entries` 只接受 `Platform`）
-- → 退避参数**永不生效**，且**只有运行时静默过滤**。
--
-- # 写通道：只走 `platform_config_upsert`（SECURITY DEFINER）
--
-- 直接 DML 已被 REVOKE（REQ-010 D56）→ 用平台级唯一写通道；
-- 它在**同一事务**里自增 `scope_key='platform'` 的 epoch → 缓存会失效 ✓。
--
-- # 默认值与 design §6 / D297 / D313 逐字一致
--
-- | 键 | 默认 | 说明 |
-- |----|------|------|
-- | `notify.maxAttempts`              | `5`    | 最多尝试次数（**含首次**） |
-- | `notify.retryBaseSeconds`         | `30`   | 第 1 次重试的延迟 |
-- | `notify.retryMultiplier`          | `2`    | 退避倍数 |
-- | `notify.retryMaxSeconds`          | `3600` | 单次延迟上限（防指数爆炸） |
-- | `notify.deliveringTimeoutSeconds` | `300`  | `delivering` 残留状态的回收阈值 |
--
-- 默认参数下的退避序列：**30s → 60s → 120s → 240s**（第 5 次失败 → 死信）。

-- 最多尝试几次（含首次）：第 5 次失败即进死信。
SELECT platform_config_upsert('notify.maxAttempts', '5'::jsonb);

-- 第 1 次重试的延迟（秒）。
SELECT platform_config_upsert('notify.retryBaseSeconds', '30'::jsonb);

-- 每次翻倍。
SELECT platform_config_upsert('notify.retryMultiplier', '2'::jsonb);

-- 单次延迟上限（防指数爆炸）。
SELECT platform_config_upsert('notify.retryMaxSeconds', '3600'::jsonb);

-- `delivering` 残留状态的回收阈值（秒）—— D313 / S-06 新增。
SELECT platform_config_upsert('notify.deliveringTimeoutSeconds', '300'::jsonb);
