# REQ-016 应用接入与应用注册表 —— 测试用例集

- **需求**: REQ-016（`acceptance.md` rev 3，11 条 AC）
- **代码基线**: `feature-REQ-016` @ `5aba742`
- **用例总数**: **61** 条（TC-016-001 ~ TC-016-061，**连续无跳号**）
- **AC 覆盖**: **11/11 = 100%**
- **配套**: 挑刺报告 `BA/feature-meta/qa-req016-spec-review.md`（39 条问题，9 阻塞）
- **标记说明**:
  - 〔AC 修正〕= 该用例给出的是**修正后**的判据（AC 原文有缺陷，见挑刺报告对应 S 编号）
  - 〔AC 补充〕= AC 未覆盖但必须验的行为
- **自动化可行性**: `集成` = 真库 + 真 HTTP（`crates/platform-app/tests/*.rs`）｜
  `单测` = 纯函数/内存 ｜ `E2E` = Playwright ｜ `命令` = shell/xtask 断言 ｜ `前端单测` = vitest

## 文件索引

| 文件 | 主题 | AC | 用例 |
|---|---|---|---|
| [01-registry.md](01-registry.md) | 注册表 CRUD / 引用 / scheme 校验 | 156、159 | TC-016-004 ~ 014、TC-016-026 ~ 029 |
| [02-isolation-lifecycle.md](02-isolation-lifecycle.md) | 租户隔离 / client 生命周期 / 撤销回归 | 157、158、024 | TC-016-001 ~ 003、TC-016-015 ~ 025 |
| [03-workbench.md](03-workbench.md) | 工作台 / 常用 / 最近 | 160、161 | TC-016-030 ~ 041 |
| [04-navigation-frontend.md](04-navigation-frontend.md) | 导航下发 / 前端运行时路由 | 162、163 | TC-016-042 ~ 052 |
| [05-audit-permission.md](05-audit-permission.md) | 审计 / 权限点 / C-5 / 对账 | 164、165 | TC-016-053 ~ 061 |

## AC → 用例映射（11/11）

| AC | 用例 | 条数 | 优先级分布 |
|---|---|---|---|
| AC-S-024 | 001、002、003 | 3 | P0×2、P1×1 |
| AC-S-156 | 004 ~ 014 | 11 | P0×8、P1×3 |
| AC-S-157 | 015 ~ 020 | 6 | P0×5、P1×1 |
| AC-S-158 | 021 ~ 025 | 5 | P0×4、P1×1 |
| AC-S-159 | 026 ~ 029 | 4 | P0×3、P1×1 |
| AC-S-160 | 030 ~ 035 | 6 | P0×4、P1×2 |
| AC-S-161 | 036 ~ 041 | 6 | P0×4、P1×2 |
| AC-S-162 | 042 ~ 047 | 6 | P0×5、P1×1 |
| AC-S-163 | 048 ~ 052 | 5 | P0×3、P1×2 |
| AC-S-164 | 053 ~ 056 | 4 | P0×3、P1×1 |
| AC-S-165 | 057 ~ 061 | 5 | P0×4、P1×1 |
| **合计** | — | **61** | P0×45、P1×16 |

## 全局前置（所有集成用例共用）

```
DATABASE_URL_TEST=postgres://base:base@127.0.0.1:5432/base_test
造数/篡改连接 = base（owner）；被测连接 = base_app（RLS 生效）
租户上下文一律「单事务内 set_config('app.tenant_id', …) → 查询」
夹具来自 scripts/seed-e2e.sh；本需求需新增见 06-夹具.md 的清单
```

> ⚠️ **跨用例隔离铁律（因 S-06）**：任何**删除 client** 的用例必须**自建一次性 client**
> （`client-cascade-probe-<n>`），**禁止**删除 `client-A` / `client-B` / `public-client`。
