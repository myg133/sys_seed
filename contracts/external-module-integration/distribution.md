# 契约分发（distribution）

> 配套 [契约正文](v1.md)。本文回答"接入方怎么拿到契约、怎么知道该升级、怎么证明拿到的是对的"。

## 1. 契约包（bundle）

每次发布产出**一个自包含的包**，接入方不需要访问平台仓库也能完成接入：

```text
external-module-v1.0.0/
├── CONTRACT.md                 # 契约正文
├── ONBOARDING.md               # 接入流程
├── CHECKLIST.md                # 准入检查表
├── CHANGELOG.md                # 本版本变更
├── VERSION                     # 发布时生成
├── SHA256SUMS                  # 发布时生成
├── schema/
│   ├── manifest.schema.json    # manifest（含 capabilities/runtime/admin）
│   ├── admin.schema.json       # 管理面描述
│   ├── config.schema.json      # descriptor + 对象 schema 子集
│   ├── audit-event.schema.json # 审计事件
│   └── problem.schema.json     # ProblemDetails
├── openapi/
│   ├── platform-api.openapi.yaml  # 应用 → 平台（配置/审计/契约/事件）
│   └── module-api.openapi.yaml    # 平台 → 模块（健康检查/事件/manifest）
├── examples/
│   ├── manifest.aibridge.json  # 完整 manifest 示例
│   ├── audit-event.json
│   └── problem.json
├── scripts/
│   └── check-examples.py       # 零依赖示例校验
├── conformance/                # 待补（P0-7/P0-8）
│   ├── docker-compose.yaml
│   ├── cases/
│   └── run.sh
└── prompts/
    ├── 01-design-review.md
    ├── 02-implement.md
    ├── 03-self-check.md
    └── 04-troubleshoot.md
```

**包的边界**：正文 + 可执行验证 + 机器可读 schema + 示例 + AI 提示词。只给 markdown 不算发布。

机器可读部分：

- JSON Schema 用 draft 2020-12，`manifest.schema.json` 通过相对 `$ref` 引用 `admin` / `config`。
- 两份 OpenAPI 3.1 **方向相反**，不要混用：`platform-api` 是应用调平台，`module-api` 是平台调模块。
- `scripts/check-examples.py` 只做关键约束检查，不是完整 validator；正式校验由 conformance 套件负责。

## 2. 命名与打包

- 目录/包名：`external-module-v<major>.<minor>.<patch>`
- 归档：`external-module-v1.0.0.tar.gz`（也提供 `.zip`）
- Git tag：`contracts/external-module/v1.0.0`
- 版本语义：
  - patch：文字修正、示例修复（不改变校验规则）
  - minor：**新增**字段/组件/能力，向后兼容
  - major：删除/改变语义，需要接入方迁移

## 3. 分发渠道

| 渠道 | 用途 | 说明 |
|---|---|---|
| **Git tag** | 主渠道 | 只读 tag；接入方可用 subtree/submodule 或直接下载 |
| **制品库** | 企业内网 | Nexus/Artifactory/OSS 镜像同一 tar.gz，保留全部历史版本 |
| **在线元数据** | CI 与工具 | `GET /api/contracts`，不含正文，只给版本、状态、校验和、下载地址 |
| **离线介质** | 隔离环境 | 同一 tar.gz + `SHA256SUMS`，人工校验 |

在线元数据示例：

```json
GET /api/contracts
{
  "items": [
    {
      "name": "external-module-integration",
      "version": "1.0.0",
      "status": "current",
      "minPlatform": "0.2.0",
      "bundle": "/api/contracts/external-module/v1.0.0.tar.gz",
      "sha256": "…",
      "schemas": {
        "manifest": "/api/contracts/external-module/v1.0.0/schema/manifest.schema.json",
        "admin": "/api/contracts/external-module/v1.0.0/schema/admin.schema.json"
      }
    },
    {
      "name": "external-module-integration",
      "version": "0.9.0",
      "status": "deprecated",
      "sunset": "2027-01-31"
    }
  ]
}
```

## 4. 版本与支持窗口

- 平台同时支持当前 minor 与上一个 minor（如 `1.1` 与 `1.0`）。
- 旧版本标记 `deprecated` 后，至少保留**一个 Sprint**（建议 4 周）。
- `sunset` 日期写进在线元数据与 CHANGELOG。
- 接入方 manifest 的 `runtime.contract` 不在支持列表 → **平台拒绝启动该模块**，并在日志里给出升级指引。

## 5. 接入方 CI

发布一个校验器（容器镜像或二进制）：

```bash
contract-verify \
  --manifest ./manifest.json \
  --contract 1.0 \
  --conformance ./conformance \
  --report ./contract-report.json
```

检查项与退出码：

| 退出码 | 含义 |
|---|---|
| 0 | 全部通过 |
| 10 | manifest 不符合 schema |
| 11 | admin 描述非法（组件/字段/权限/路径） |
| 12 | pin 的契约版本不受支持 |
| 13 | 契约测试有失败项 |
| 14 | 审计/配置样例不符合契约 |

CI 建议：

- PR 阶段：schema + admin 校验（快）
- 合并前：契约测试（起容器，较慢）
- 发布前：完整报告归档

## 6. 变更通知

三层，缺一不可：

1. **CHANGELOG.md**：每个版本的变更、影响面、迁移步骤。
2. **在线元数据**：`status` 与 `sunset` 字段，工具可自动读取。
3. **主动通知**：平台发布时向已登记接入方发邮件/Webhook（地址来自应用注册表），内容含版本、影响、截止日期。

弃用流程：

```text
1.0 current → 1.1 发布（1.0 仍 current）
→ 1.1 稳定后 1.0 标 deprecated（写 sunset）
→ 平台同时支持 1.0 / 1.1
→ sunset 前 4 周再次通知
→ sunset 后拒绝 1.0（启动失败，给升级指引）
```

## 7. 完整性与安全

- 包内附 `SHA256SUMS`；接入方下载后必须校验。
- 企业环境可附 cosign/sigstore 签名；公网分发时建议启用。
- 契约包**不含**任何密钥、真实租户数据或内部地址。
- 在线元数据端点只读、可缓存（`ETag`），不要求认证；schema 可匿名获取。

## 8. 离线 / 内网场景

- 提供同一 tar.gz 与 `SHA256SUMS`；不依赖在线端点。
- `contract-verify` 支持完全离线（镜像内置 schema 与测试用例）。
- 示例模块可在无外网环境构建（依赖已 vendor）。

## 9. 发布检查清单（平台方）

- [ ] 正文与 schema 一致（字段名、枚举、必填项）
- [ ] 示例模块能跑通契约测试
- [ ] 测试套件对旧版本仍可运行（回归）
- [ ] CHANGELOG 写清 breaking 项与迁移步骤
- [ ] 在线元数据已更新（version/status/sunset/sha256）
- [ ] 制品库与 Git tag 指向同一份内容
- [ ] 通知已发给全部已登记接入方
- [ ] 平台侧的版本兼容矩阵已更新
