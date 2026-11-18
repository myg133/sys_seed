# REQ-006 状态

- **需求**: REQ-006 多租户与隔离
- **分支**: `feature/REQ-006`
- **基线**: `develop` @ `806bb66`（REQ-002 + REQ-003 + REQ-004 + REQ-005 已合并）
- **状态**: **待验证**（开发完成 → 等待 QA 复核；本状态由 Dev 按任务要求置位，**最终状态机由 BA 推进**）
- **开发者**: myg133-dev-1
- **完成时间**: 2026-11-18
- **产物**: `feature-meta/traceability.md`、`feature-meta/verification-report.md`
- **设计依据**: `BA/demands/REQ-006-tenant/design-summary.md`（D1–D8 照做）

## 本轮交付

租户隔离做**双保险**：应用层每次查询都带租户条件，数据库层用 PostgreSQL RLS 再兜一遍。

| 交付项 | 位置 |
|--------|------|
| 新建 `crates/platform-db`：租户作用域入口（事务内 `SET LOCAL app.tenant_id`）+ 配额计数/限额查询 | `crates/platform-db/src/` |
| 平台表与业务表的 RLS 策略（业务表由 manifest 的 `tenantScoped` 段**统一生成**） | `migrations/20261118000200_tenant_isolation.up.sql`、`crates/platform-db/src/rls.rs`、`cargo xtask tenant-policies` |
| manifest 新增 `tenantScoped` 段 + JSON Schema 同步（**`platform-core`，CODEOWNERS 保护目录**） | `crates/platform-core/src/extension/manifest.rs`、`manifest_schema.rs` |
| C-3 契约检查**叠加真实库层**（内存层保留）+ 串租户用例 | `modules/sandbox/tests/c3_real_db.rs` |
| 沙箱模块升级为「业务方照着抄」的标准范例 | `modules/sandbox/backend/src/tenant_repository.rs` |
| 能力索引 / 文档 / 依赖门禁收录 `platform-db` | `crates/xtask/src/{catalog,deps,tenant_policies}.rs`、`AGENTS.md`、`docs/dev-environment.md`、`.github/CODEOWNERS` |

## 自验结论

- `cargo check --workspace --all-targets` ✅
- `cargo test --workspace -- --test-threads=2` ✅ **170/170**（REQ-005 基线 142 个全绿，**+28 只增不减**）
- 真实库用例（默认 `#[ignore]`，由集成脚本 `--include-ignored` 跑到）✅ **3/3**
- `bash scripts/test-integration.sh` ✅ **exit 0**（重置 `base_test`，容器数量 2 → 2；**连跑 2 次全绿**）
- `cargo clippy --workspace --all-targets -- -D warnings` ✅ **0 告警** / `cargo fmt --all --check` ✅
- `pnpm -r test` ✅ **23/23** / `bun run test:e2e` ✅ **1/1**
- `cargo xtask capabilities --check` ✅ 13 文件一致 / `check-deps` ✅ / `check-modules` ✅ / `tenant-policies --check` ✅
- **依赖方向门禁实测**：临时给 `platform-db` 加反向依赖 → `check-deps` **退出码 1** 并点名 `platform-db → sandbox` → **验证后已还原**
- **租户隔离三层** ✅ 应用层（`{tenant}` 强制）/ **RLS fail closed**（不设 `app.tenant_id` → **0 行**，管理员连接对照 **4 行**）/ **串租户**（同一条物理连接，tenant_a 查完查 tenant_b → 看不到 tenant_a 的行）
- **冷编译实测**：**63.7s / 66.0s**，峰值 **925 MB / 940 MB** → **< 1.5G，`jobs` 保持 3**（无需下调）
- **依赖树**：175 → **179**（工作区成员 6 → 7；**第三方 172，+0**；`sqlx-macros` 不参与编译）
- **新增第三方依赖：无**

## 改动了 `platform-core`（CODEOWNERS 门禁范围）

- `manifest.rs` +138 / `manifest_schema.rs` +76 / `mod.rs` +1 —— **纯新增，0 删除**
- **理由**：需求 G1 要求模块的租户作用域声明写在 manifest 里，平台才有地方读；
  manifest 的 schema 是**对外发布**的，必须与平台实现完全一致
- **门禁影响**：会触发人工评审（预期内）；`platform-contracts` / `platform-contracts-macros` **一行未改**

## 本轮修掉的一个真问题

策略应用原先把 `DROP POLICY` + `CREATE POLICY` 分两条语句执行 →
中间有「表上没有 policy」的窗口（RLS 变成全部拒绝），并发访问时表现为
「表是空的」假象（10 次并发跑 3 次偶发失败）。**已改为在一个事务里应用**，
复测 10/10 全绿。详见验证报告 §4。

## 未验证 / 未完成（如实登记）

1. **AC-S-008（缓存 / 消息链路的租户维度）未实现** —— 本期设计边界不含（`design-summary` §8）
2. **部署流程（compose / Helm）尚未调用 `cargo xtask tenant-policies`** —— 命令已就绪但未接线（避免扩大 scope）
3. **覆盖率未测** —— 低配机无覆盖率工具链
4. **`base_dev` 上的真实验证未做** —— 本轮数据库验证全部跑在 `base_test`（开发库未被触碰）
5. **真实负载下的性能未量化** —— 每次访问多一次 `BEGIN`/`COMMIT`（设计取舍）
6. **应用连接角色（普通角色）在生产部署里的接入未验证** —— 需部署流程落实

## 请派 QA 复核

重点看验证报告 §7 的 5 条：RLS fail closed 的对照组、串租户用例的强度、
策略应用窗口的修法、`platform-core` 的 schema 扩展、部署流程缺口（R2）。
