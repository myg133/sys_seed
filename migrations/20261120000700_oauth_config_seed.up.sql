-- 出站 IdP 的 5 个配置项默认值（REQ-022，design §7）。
--
-- # 登记要**两处**都有（漏 INSERT → 平台起不来）
--
-- 1. `crates/platform-oauth/manifest.json` 的 `config` 段 —— 键的**登记**
--    （注册表在**启动时**扫描 manifest；没登记 → 写不进去）；
-- 2. 本迁移 —— 值的**落库**（平台级默认值写在 `platform_config`）。
--
-- # ⚠️ 键名是 **camelCase**（契约 C9）
--
-- `ConfigKey::parse` 要求每一段「**小写字母开头 + 只含字母数字**」
-- （`crates/platform-core/src/config.rs`）→ **带 `_` 的键名直接让服务起不来**
-- → 所以是 `oauth.frontendBaseUrl`（**不是** `oauth.frontend_base_url`）。
--
-- # 写通道：只走 `platform_config_upsert`（SECURITY DEFINER）
--
-- 直接 DML 已被 REVOKE（REQ-010 D56）→ 用平台级唯一写通道；
-- 它在**同一事务**里自增 `scope_key='platform'` 的 epoch → 缓存会失效 ✓。
--
-- # 默认值与 design §7 逐字一致
--
-- | 键 | 默认 | 说明 |
-- |----|------|------|
-- | `oauth.frontendBaseUrl` | `""` | 授权页 / 同意页的前端基址；**空 = 用 `{issuer}`**（D222） |
-- | `oauth.accessTokenTtlSeconds` | `3600` | **撤销延迟上界**（D210） |
-- | `oauth.authorizationCodeTtlSeconds` | `60` | 授权码 TTL |
-- | `oauth.deviceCodeTtlSeconds` | `600` | 设备码 TTL |
-- | `oauth.deviceCodePollIntervalSeconds` | `5` | 设备码轮询间隔 |

-- 授权页 / 同意页的前端基址（空串 = 回退到 `{issuer}`，D222）。
SELECT platform_config_upsert('oauth.frontendBaseUrl', '""'::jsonb);

-- access token 有效期（秒）—— 撤销延迟的上界（D210）。
SELECT platform_config_upsert('oauth.accessTokenTtlSeconds', '3600'::jsonb);

-- 授权码有效期（秒）。
SELECT platform_config_upsert('oauth.authorizationCodeTtlSeconds', '60'::jsonb);

-- 设备码有效期（秒）。
SELECT platform_config_upsert('oauth.deviceCodeTtlSeconds', '600'::jsonb);

-- 设备码轮询间隔（秒）。
SELECT platform_config_upsert('oauth.deviceCodePollIntervalSeconds', '5'::jsonb);
