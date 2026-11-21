-- 应用接入的 3 个配置项默认值（REQ-016，design §6 / D274）。
--
-- # 登记要**两处**都有（漏 INSERT → 平台起不来 / 配置读不到）
--
-- 1. `crates/platform-app/manifest.json` 的 `config` 段 —— 键的**登记**
--    （注册表在**启动时**扫描 manifest；没登记 → 写不进去）；
-- 2. 本迁移 —— 值的**落库**（平台级默认值写在 `platform_config`）。
--
-- # ⚠️ 键名是 **lowerCamelCase**（契约 C9）
--
-- `ConfigKey::parse` 要求每一段「**小写字母开头 + 只含字母数字**」
-- （`crates/platform-core/src/config.rs`）→ **带 `_` 的键名直接让服务起不来**。
--
-- # ⚠️ 三个键都是 **`scope: "platform"`**（D276 / QA 的 S-01）
--
-- manifest 里三个键必须写 `"scope": "platform"` —— `ConfigSpec.scope` 的 serde
-- 默认值是 **`Tenant`** → 漏写就落到租户作用域 → 平台级读写端点直接拒
-- （`platform-config/src/service.rs` 的 `read_platform_entries` 只接受 `Platform`）
-- → AC-S-160 / AC-S-161 的「配置生效」**永不可达**，且**只有运行时静默过滤**。
--
-- ⚠️ `platform_config_upsert` 会按 manifest 里的 `scope` 决定落的行
-- （本迁移不显式写 `scope` 列 —— 由函数处理；三个键的 scope 来自 manifest）。
--
-- # 写通道：只走 `platform_config_upsert`（SECURITY DEFINER）
--
-- 直接 DML 已被 REVOKE（REQ-010 D56）→ 用平台级唯一写通道；
-- 它在**同一事务**里自增 `scope_key='platform'` 的 epoch → 缓存会失效 ✓。
--
-- # 默认值与 design §6 / D274 逐字一致
--
-- | 键 | 默认 | 说明 |
-- |----|------|------|
-- | `app.recentCount` | `6` | 「最近访问」与「常用应用」各取前 N |
-- | `app.workspace.maxApps` | `60` | 工作台最多展示的应用数 |
-- | `app.icon.allowHttp` | `false` | URL 是否允许 `http`（仅本地开发） |

-- 最近访问 / 常用应用各取前 N 条。
SELECT platform_config_upsert('app.recentCount', '6'::jsonb);

-- 工作台最多展示的应用数（防一屏塞爆）。
SELECT platform_config_upsert('app.workspace.maxApps', '60'::jsonb);

-- URL 是否允许 `http`（仅本地开发；生产必须 https）。
SELECT platform_config_upsert('app.icon.allowHttp', 'false'::jsonb);
