-- 出站 IdP 的 5 个配置项默认值回滚（REQ-022，design §7）。
--
-- 只删**本迁移**登记的那 5 个键 —— 不碰任何其它配置行。
--
-- ⚠️ `platform_config` 的直接 DML 已被 REVOKE（REQ-010 D56），
-- 但**迁移跑在表 owner 上** → 可以直接 DELETE（回滚是 owner 权限的活）。

DELETE FROM platform_config
 WHERE key IN (
       'oauth.frontendBaseUrl',
       'oauth.accessTokenTtlSeconds',
       'oauth.authorizationCodeTtlSeconds',
       'oauth.deviceCodeTtlSeconds',
       'oauth.deviceCodePollIntervalSeconds'
 );
