# 沙箱模块数据库迁移

本目录放**模块自己的建表迁移**（`20261118000300_sandbox_tenant_item.up.sql` +
配套 `.down.sql`）。

## 迁移文件里**只有建表**，没有 RLS policy

租户作用域表的 policy **由平台统一生成**（`manifest.json` 的 `tenantScoped` 段 →
`cargo xtask tenant-policies`）。模块手写 policy 迟早会漏 ——
**漏一张表 = 该表完全无隔离**（不是「少一个条件」，是「谁都能查」）。

## 命名与可逆性

- 文件名与平台迁移目录保持一致：`<版本>_<描述>.up.sql` + `<版本>_<描述>.down.sql`；
- 每个迁移必须配套 `down`，否则需在 PR 里标注 ⚠️ 不可逆。

## 规则何时升级

REQ-005 时期底座还没有业务 DB 层，`migrations/` 只放说明文件也算合规，
并写明「**DB 层落地后规则将升级为至少一个 `*.sql`**」。

**REQ-006 起 DB 层已落地**（`crates/platform-db`）——
`cargo xtask check-modules` 的校验口径已升级为「`migrations/` 下至少有一个 `*.sql` 迁移文件」。
本说明文件可以删除。
