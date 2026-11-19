# 状态：REQ-012 元数据驱动渲染

- **状态**: 已完成
- **更新者**: myg133-ba-1 (mcp:proc-83964)
- **更新时间**: 2026-11-19 18:35
- **合并点**: `develop` @ `88b7ca3`

## 状态历史

| 时间 | 从 | 到 | 操作者 | 备注 |
|------|----|----|--------|------|
| 2026-11-17 23:45 | — | 草稿 | myg133-ba-1 | 从 REQ-001 拆分创建 |
| 2026-11-19 15:25 | 草稿 | 已就绪 | myg133-ba-1 | 规格写实：AC-S-076~087（12 条）+ 设计摘要。范围经用户拍板：第一棒渲染闭环 + 字段级权限（脚手架延后）；字段级权限走「平台唯一序列化入口 + 契约 C-6」 |
| 2026-11-19 15:45 | 已就绪 | 开发中 | myg133-ba-1 | QA 挑刺 **40 条**（高 12，含 **3 条「主体不存在」**）→ 裁定 39/1 → rev 2（D102~D106） |
| 2026-11-19 16:50 | 开发中 | 开发中 | myg133-ba-1 | Dev 提偏差 6 项全部接受 + **D107**（加 demoNote 验 AC-S-085） |
| 2026-11-19 17:35 | 开发中 | 修复中 | myg133-ba-1 | Pre-merge **FAIL**（2 HIGH：测试资产不可用）→ 裁定 + **D108** |
| 2026-11-19 18:15 | 修复中 | 修复中 | myg133-ba-1 | 修复轮（E2E 17 passed / 0 skipped）→ 独立复核 **PASS** |
| 2026-11-19 18:35 | 修复中 | **已完成** | myg133-ba-1 | 收尾 3 条 LOW + 合并 `develop` @ `88b7ca3` |

## 阻塞项

无。

## 交付内容

| 层 | 内容 |
|---|---|
| 元数据声明 | `manifest.json` 的 `schema` 段（实体/字段/**api 子段**）+ 两阶段校验 |
| 元数据接口 | `GET /api/schema/<module>/<entity>`，无权字段连存在都不暴露 |
| 字段级权限 | `SchemaProjector`（白名单 fail closed）+ 契约 **C-6** |
| 示例模块 | `sandbox` 挂进 `platform-server` + `demoItem` / `demoNote` 完整 CRUD |
| 前端 | `@base/schema` 纯逻辑 + `SchemaTable`/`SchemaForm`/`SchemaPage` + router `:param` |

## 验收结果

**12/12 条 AC 通过**（独立复核逐条真机/真浏览器验证）

## 关键数字

| 项 | 值 |
|---|---|
| 后端集成 | **618 passed / 0 failed**（33 套件，post-merge 复跑） |
| 前端单测 | **240 passed** |
| E2E | **17 passed / 0 skipped**（正序 + 反序都绿） |
| 依赖预算 | `Cargo.lock` **296**（+1 本地新 crate）；**第三方 284 不变** |
| 决策 | **D102 ~ D108**（7 条） |

## 验证链

```
QA 挑刺 40 条（3 条「主体不存在」+ 3 条 BA 自错）→ 裁定 39/1（rev 2）
  → Dev 双棒并行 → 偏差 6 项裁定（D102~D107）
  → QA Pre-merge FAIL（2 HIGH：夹具没跟上权限点 / 用例互斥）
  → 修复轮 → 独立复核 PASS → BA 收尾 3 条 LOW → 合并
```

## 文档

- [demand.md](demand.md) / [acceptance.md](acceptance.md)（rev 3）/ [design-summary.md](design-summary.md)
- [acceptance-rev2-ruling.md](acceptance-rev2-ruling.md) —— 40 条挑刺的裁定
- [deviation-ruling.md](deviation-ruling.md) —— 实现期 6 项偏差 + D107
- [qa-fail-ruling.md](qa-fail-ruling.md) —— Pre-merge FAIL 的裁定 + D108
- [test-cases/](test-cases/) —— 60 条用例
- [qa-report.md](qa-report.md)（Pre-merge FAIL）/ [qa-report-recheck.md](qa-report-recheck.md)（复核 PASS）
- [dev-report-fix.md](dev-report-fix.md)
- 复盘：[req-012-retro.md](../../sprint/req-012-retro.md)

## 关键教训

1. **「主体不存在」是需求最大的风险** —— 3 条（数据接口地址 / 挂载的模块 / 通配路由）
   都只能靠「把整条链路走一遍」发现
2. **测试资产会腐烂（第三次）** —— 夹具没跟上新权限点 → 6 条 E2E 全 skip、exit 0
3. **`skip` 是假绿，最隐蔽** —— D108 定了判定表
4. **AC 可以有「物理不可达」的前置** —— `input[type=number]` 不接受 `abc`
