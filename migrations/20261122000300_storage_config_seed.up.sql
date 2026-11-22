-- 存储与导入导出的 6 个配置项默认值（REQ-018，design §6 / D319~D332）。
--
-- # 登记要**两处**都有（漏一处 → 平台起不来 / 配置读不到）
--
-- 1. `crates/platform-storage/manifest.json` 的 `config` 段 —— 键的**登记**
--    （注册表在**启动时**扫描 manifest；没登记 → 写不进去）；
-- 2. 本迁移 —— 值的**落库**（平台级默认值写在 `platform_config`）。
--
-- # ⚠️ 键名是 **lowerCamelCase**（契约 C9）
--
-- `ConfigKey::parse` 要求每一段「**小写字母开头 + 只含字母数字**」→
-- **带 `_` 的键名直接让服务起不来**（本迁移 6 个键全部合规）。
--
-- # ⚠️ 6 个键都必须是 **`scope: "platform"`**
--
-- manifest 里 `ConfigSpec.scope` 的 serde 默认值是 **`Tenant`** → 漏写就
-- 落到租户作用域 → 平台级读取端点直接拒 → 上限/批次参数**永不生效**，
-- 且是**运行时静默过滤**（同 REQ-015 的 D304 教训）。
--
-- # 写通道：只走 `platform_config_upsert`（SECURITY DEFINER）
--
-- `platform_config` 的直接 DML 已被 REVOKE（REQ-010 D56）→ 用平台级唯一写
-- 通道；它在**同一事务**里自增 `scope_key='platform'` 的 epoch → 缓存失效 ✓。
--
-- # 默认值与 design §6 逐字一致
--
-- | 键 | 默认 | 说明 |
-- |----|------|------|
-- | `storage.maxUploadBytes`          | `104857600` | 单对象上限 100 MiB |
-- | `storage.partSizeBytes`           | `8388608`   | 单片 8 MiB |
-- | `storage.presignTtlSeconds`       | `900`       | 预签名链接有效期 15 分钟 |
-- | `storage.importBatchSize`         | `1000`      | 导入每批行数 |
-- | `storage.maxRowBytes`             | `65536`     | 单行原始字节上限 64 KiB |
-- | `storage.uploadSessionTtlSeconds` | `86400`     | 未完成上传会话存活 24 小时 |

-- 单对象大小上限（字节）：超出直接拒，不进存储。
SELECT platform_config_upsert('storage.maxUploadBytes', '104857600'::jsonb);

-- 分片大小（字节）：与 maxUploadBytes 共同决定分片数上限。
SELECT platform_config_upsert('storage.partSizeBytes', '8388608'::jsonb);

-- 预签名有效期（秒）：越短越安全，太长则大文件续传会中途失效。
SELECT platform_config_upsert('storage.presignTtlSeconds', '900'::jsonb);

-- 导入批次行数：一批一事务，批次越大吞吐越高、单批回滚代价越大。
SELECT platform_config_upsert('storage.importBatchSize', '1000'::jsonb);

-- 单行原始字节上限：防「一行超长」把解析器的内存吃光（CSV 的经典攻击面）。
SELECT platform_config_upsert('storage.maxRowBytes', '65536'::jsonb);

-- 未完成上传会话的存活时长（秒）：与 expires_at 配合，由 TTL 扫描器回收。
SELECT platform_config_upsert('storage.uploadSessionTtlSeconds', '86400'::jsonb);
