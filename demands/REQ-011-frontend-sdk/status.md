# 状态：REQ-011 前端 SDK 骨架

- **状态**: 已完成
- **更新者**: myg133-ba-1 (mcp:proc-83964)
- **更新时间**: 2026-11-19 15:05
- **合并点**: `develop` @ `b38715f`

## 状态历史

| 时间 | 从 | 到 | 操作者 | 备注 |
|------|----|----|--------|------|
| 2026-11-17 23:45 | — | 草稿 | myg133-ba-1 | 从 REQ-001 拆分创建 |
| 2026-11-19 10:50 | 草稿 | 已就绪 | myg133-ba-1 | 规格写实：AC-S-064~075（12 条）+ 设计摘要 |
| 2026-11-19 11:05 | 已就绪 | 开发中 | myg133-ba-1 | QA 挑刺 **40 条**全部接受 → rev 2（D92~D95） |
| 2026-11-19 12:40 | 开发中 | 修复中 | myg133-ba-1 | Pre-merge **FAIL**（1 HIGH 壳层登录 100% 失败 + AC-S-068 步骤 3）→ rev 3（D98/D99） |
| 2026-11-19 13:30 | 修复中 | 修复中 | myg133-ba-1 | 独立复核 1 **PASS-WITH-RISK**（发现 2 个新 MEDIUM）→ rev 4（D100/D101） |
| 2026-11-19 14:50 | 修复中 | **已完成** | myg133-ba-1 | 独立复核 2 **PASS**；修复轮 3 收尾 2 个遗留；合并 `develop` @ `b38715f` |

## 阻塞项

无。

## 交付内容

| 层 | 内容 |
|---|---|
| 后端 | `PermissionProvider` trait（`platform-core`）+ `GET /api/rbac/me/permissions` + 登录响应填真实权限点 |
| 前端 | `@base/sdk` / `@base/auth` / `@base/theme` / `@base/ui` / `@base/hooks`（从占位到实现） |
| 壳 | 登录页（租户输入）+ 极简路由 + 受保护页 + 403 页 + `<Can>` + `sessionStorage` 会话 |
| 测试 | 后端集成 10 条 + 前端单测 151 条 + E2E 11 条（入库） |

## 验收结果

**12/12 条 AC 通过**（两轮独立复核，全部有真机/真浏览器证据）

## 验证链（7 棒，每棒都抓到东西）

```
BA 规格 → QA 挑刺 40 条（3 条是 BA 自己的错，全部接受）→ Dev 实现
  → Pre-merge FAIL（HIGH：壳层登录 100% 失败，单测全绿）
  → 修复轮 1（租户来源 + 会话持久化）
  → 独立复核 1 PASS-WITH-RISK（2 个新 MEDIUM：错误码被覆盖 / E2E 夹具不可复现）
  → 修复轮 2
  → 独立复核 2 PASS（确认修好 + 未破坏已通过的 8 项）
  → 修复轮 3（2 个遗留）→ 合并
```

## 关键数字

| 项 | 值 |
|---|---|
| 后端回归 | **550 passed / 0 failed**（29 套件，连跑 2 轮 + post-merge） |
| 前端单测 | **151 passed** |
| E2E | **11 passed**（入库） |
| 独立探针（QA 自建） | **61 条**（浏览器 29 + SDK 32） |
| 依赖预算 | `Cargo.lock` **295 包**（diff = 0）；pnpm 零第三方新增 |
| 决策 | **D92 ~ D101**（10 条） |

## 文档

- [demand.md](demand.md) / [acceptance.md](acceptance.md)（rev 4）/ [design-summary.md](design-summary.md)
- [acceptance-rev2-ruling.md](acceptance-rev2-ruling.md) —— 40 条挑刺的逐条裁定
- [test-cases/](test-cases/) —— 48 条用例
- [qa-report.md](qa-report.md) —— Pre-merge（FAIL）
- [qa-report-recheck.md](qa-report-recheck.md) —— 独立复核 1（PASS-WITH-RISK）
- [qa-report-recheck2.md](qa-report-recheck2.md) —— 独立复核 2（PASS）
- [dev-report-fix2.md](dev-report-fix2.md) —— 修复轮 2 自验
- 复盘：[sprint-04-retro.md](../../sprint/sprint-04-retro.md)

## 顺带修掉的既有缺陷（非本需求范围）

1. **theme 包的无扩展名导入** → Node ESM 加载不了，违反「五个包可独立引入」
2. **`prettier --check .` 在基线就红**（2 个 manifest.json）→ 会让 CI lint job 永远红
3. **CI 的 test job 无 postgres service** → `cargo test --workspace` 必红（TD-004）
4. **CI 的 test job 不 build** → `packaging.test.ts` 的 `skipIf(!built)` 让 AC-S-075 的
   2 条产物用例永远 skip
5. **★ 80 条 `#[ignore]` 用例在 CI 永远不跑**（含本需求的 10 条后端验收）
   → 新增 `integration` job
6. **`test-integration.sh` 在无 psql/无 podman 时静默跳过但 exit 0**
   → 加 `cannot_reset()`：本地跳过，CI 拒绝静默跳过

## 未解风险

- **CI 仍从未真跑**（无 remote，TD-006）—— 上面 6 条修复都**只是配好了**，
  没有一次真实执行验证
- 分支保护需人工配置（TD-007）
