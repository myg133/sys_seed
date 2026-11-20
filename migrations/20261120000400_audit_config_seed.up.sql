-- 审计配置项的**默认值**（REQ-014 D178）。
--
-- # 登记要**两处**都有（漏 INSERT → 平台起不来）
--
-- 1. `crates/platform-config/manifest.json` 的 `config` 段 —— 键的**登记**
--    （注册表在**启动时**扫描 manifest；没登记 → 写不进去）；
-- 2. 本迁移 —— 值的**落库**（平台级默认值写在 `platform_config`）。
--
-- # ⚠️ 键名是 **camelCase**（契约 C9），**不是** D178 正文的 snake_case
--
-- D178 正文写的是 `audit.retention.online_days` / `audit.page.default_size` 等
-- **snake_case** 名字 → **与契约 C9 冲突**（C9：「键名 **camelCase**，层级用 `.` 分隔」）。
--
-- 实测：配置注册表在**启动时**用 `ConfigKey::parse` 校验每一个键
-- （`crates/platform-config/src/registry.rs`），而它要求每一段
-- 「**小写字母开头 + 只含字母数字**」→ **带 `_` 的键名直接让服务起不来**
-- （`ConfigError::KeyInvalid`）。
--
-- → 本迁移按**可用的** camelCase 登记：
--
-- | D178 正文（snake_case） | 本次登记（camelCase） |
-- |------------------------|----------------------|
-- | `audit.retention.online_days` | `audit.retention.onlineDays` |
-- | `audit.retention.archive_days` | `audit.retention.archiveDays` |
-- | `audit.page.default_size` | `audit.page.defaultSize` |
-- | `audit.page.max_size` | `audit.page.maxSize` |
--
-- ⚠️ D178 正文的 snake_case 写法**与契约 C9 冲突**，**D181 已裁定改用 camelCase**，
-- D178 的 snake_case 写法**作废**。
--
-- # 写通道：只走 `platform_config_upsert`（SECURITY DEFINER）
--
-- 直接 DML 已被 REVOKE（REQ-010 D56）→ 用平台级唯一写通道；
-- 它在**同一事务**里自增 `scope_key='platform'` 的 epoch → 缓存会失效 ✓。
--
-- # 本轮**只登记不执行**
--
-- `audit.retention.archive_days`（归档记录的物理删除阈值）**不执行** ——
-- 归档策略是**手写**的（D166），且物理删除涉及不可逆操作，本轮不做。

-- 在线保留天数：超期记录由归档搬进归档表。
SELECT platform_config_upsert('audit.retention.onlineDays', '90'::jsonb);

-- 归档保留天数：归档记录的物理删除阈值（本轮只登记，不执行）。
SELECT platform_config_upsert('audit.retention.archiveDays', '365'::jsonb);

-- 列表默认每页条数。
SELECT platform_config_upsert('audit.page.defaultSize', '20'::jsonb);

-- 列表每页条数上限。
SELECT platform_config_upsert('audit.page.maxSize', '200'::jsonb);
