# 设计摘要：文件存储与导入导出（REQ-018）

- **需求编号**: REQ-018
- **状态**: rev 1（2026-11-22）
- **基线代码**: `develop` @ `c27528a`
- **决策依据**: [decisions.md](decisions.md)（D319~D332）
- **验收标准**: [acceptance.md](acceptance.md)（**11 条**：AC-S-026 重写 + AC-S-175~184）

---

## 1. 架构与依赖

```
              ┌──────────────────────────────────────┐
              │  platform-storage/http.rs            │  multipart（D321）
              │  authz.rs（3 个权限点）              │
              └───────────────┬──────────────────────┘
                              │
              ┌───────────────▼──────────────────────┐
              │  service.rs（编排）                  │
              │  上传会话 / 导入导出任务              │
              └──┬──────────┬──────────────┬─────────┘
                 │          │              │
      ┌──────────▼──┐  ┌────▼───────┐  ┌───▼──────────┐
      │object_store/│  │ csv.rs     │  │ store.rs     │
      │  trait      │  │ RFC 4180   │  │ TenantTx     │
      └──┬───────┬──┘  └────────────┘  └──────────────┘
         │       │
   ┌─────▼──┐ ┌──▼─────┐        ┌──────────────────┐
   │local_fs│ │ s3.rs  │        │ worker.rs tick() │
   │ ✅ 真  │ │⚠️协议  │        │ SKIP LOCKED      │
   └────────┘ └────────┘        └──────────────────┘
              ┌──────────────────────┐
              │ presign.rs（HMAC）   │
              │ scanner.rs（钩子）   │
              └──────────────────────┘
```

### crate 依赖

```toml
[dependencies]
platform-core.workspace = true
platform-db.workspace = true
platform-config.workspace = true
# ⚠️ 第二次依赖破例（D321）：multipart → multer 3.0（净增 multer + encoding_rs）
axum = { workspace = true, features = ["multipart"] }
tokio / serde / serde_json / tracing / thiserror / uuid / time
# SigV4 与预签名（**不是新增依赖**：已在 Cargo.lock，D329）
hmac = "0.12"
sha2.workspace = true
base64.workspace = true
# 常数时间比较（防时序侧信道，D322）
subtle = "2"
bytes.workspace = true
```

> ⚠️ `hmac` 与 `subtle` 都**已在 `Cargo.lock`**（前者由 `jsonwebtoken`/`totp-rs` 传递引入，
> 后者由 `ring`/`rustls` 传递引入）→ 加 workspace 条目**不新增 lock 条目**。

---

## 2. 数据模型（**4 张表**）

### 2.1 `platform_storage_object` —— 对象元数据（**租户级**，RLS）

| 列 | 类型 | 说明 |
|---|---|---|
| `id` | `uuid` | PK |
| `tenant_id` | `text` | |
| `object_key` | `text` | 逻辑路径（`docs/a.txt`） |
| `size_bytes` | `bigint` | |
| `sha256` | `text` | 内容摘要（**合并后校验**用） |
| `content_type` | `text` | |
| `scan_verdict` | `text` | `skipped` / `clean` / `infected`（D326） |
| `status` | `text` | `pending` / `available` / `deleting` |
| `created_at` / `updated_at` | `timestamptz` | |

```sql
UNIQUE (tenant_id, object_key)
```

### 2.2 `platform_storage_upload` —— 上传会话（**租户级**，RLS）

| 列 | 说明 |
|---|---|
| `upload_id`（PK）· `tenant_id` · `object_key` | |
| `total_parts` · `part_size` | |
| `declared_checksum` | 客户端声明（`complete` 时比对） |
| `status` | `pending` / `completing` / `completed` / `aborted` |
| `created_at` / `updated_at` / `expires_at` | |

**part 不单独建表**：分片数据直接落存储（`LocalFsStore` 写成
`<root>/.uploads/<upload_id>/part-<n>`），**元数据就是文件是否存在**
→ 「哪些片已传」= 列目录。这样幂等覆盖天然成立（同名文件覆盖）。

### 2.3 `platform_storage_job` —— 导入/导出任务（**租户级**，RLS）

| 列 | 说明 |
|---|---|
| `id`（PK）· `tenant_id` | |
| `kind` | `import` / `export` |
| `entity` | 目标实体（`probe_rows` 等） |
| `status` | `pending` / `running` / `completed` / `failed` |
| `total_rows` · `processed_rows` · `failed_rows` | **进度**（每批更新，D325） |
| `object_key` | 导入的源 / 导出的结果 |
| `error` | 整体失败原因（行级错在下一张表） |
| `created_at` / `updated_at` / `finished_at` | |

### 2.4 `platform_storage_job_error` —— 行级错误回执（**租户级**，RLS）

| 列 | 说明 |
|---|---|
| `job_id` · `tenant_id` · `row_number` | `row_number` 是**文件行号**（1-based） |
| `column` | 真实列名（`email` / `amount`），不是下标 |
| `reason` | **英文 ASCII**（中文文案由前端 i18n） |

```sql
PRIMARY KEY (job_id, row_number, column)
```

### 2.5 RLS

四张表全部在 manifest 的 `tenantScoped.tables` 里 → `xtask tenant-policies` 生成策略。

⚠️ **AC-S-183 要求 4 张表各验一次**（不是只验一张）——
manifest 漏声明某张表时 `--check` **会绿**（没声明就不生成也不校验）。

---

## 3. 端点清单（**11 个**）

| # | 方法 | 路径 | 权限 |
|---|---|---|---|
| 1 | `PUT` | `/api/storage/objects/{key}` | `storage:file:write` |
| 2 | `GET` | `/api/storage/objects/{key}` | `storage:file:read` |
| 3 | `DELETE` | `/api/storage/objects/{key}` | `storage:file:write` |
| 4 | `GET` | `/api/storage/objects` | `storage:file:read` |
| 5 | `POST` | `/api/storage/objects/{key}/presign` | `storage:file:read` |
| 6 | `POST` | `/api/storage/uploads` | `storage:file:write` |
| 7 | `PUT` | `/api/storage/uploads/{id}/parts/{n}` | `storage:file:write` |
| 8 | `POST` | `/api/storage/uploads/{id}/complete` | `storage:file:write` |
| 9 | `DELETE` | `/api/storage/uploads/{id}` | `storage:file:write` |
| 10 | `POST` | `/api/storage/imports` · `/exports` | `storage:file:write` |
| 11 | `GET` | `/api/storage/jobs` · `/jobs/{id}` · `/jobs/{id}/errors` | `storage:job:read` |

**+1 个免鉴权端点**：`GET /api/storage/download/{token}`（预签名下载，**靠签名而非会话**）。

### 3.1 跨栈字段（camelCase，D258）

| JSON | 列 |
|---|---|
| `objectKey` | `object_key` |
| `sizeBytes` · `sha256` · `contentType` | 同名列 |
| `scanVerdict` | `scan_verdict` |
| `uploadId` · `totalParts` · `partSize` | 同名列 |
| `jobId` · `kind` · `totalRows` · `processedRows` · `failedRows` | 同名列 |
| `rowNumber` · `column` · `reason` | 同名列 |

---

## 4. 关键流程

### 4.1 预签名令牌（D322）

```
签发：payload = key_id "." expires_at_unix "." object_key
      sig     = HMAC-SHA256(secret, payload)
      token   = base64url(payload) "." base64url(sig)

校验（顺序不可换）：
  ① 解析 payload → 取 key_id / expires_at / object_key
  ② 重算 sig，与 token 里的 sig 做 **constant_time_eq**
     ↳ 不符 → 403 STORAGE_PRESIGN_INVALID
  ③ expires_at < now → 403 STORAGE_PRESIGN_EXPIRED
  ④ 读文件
```

⚠️ **② 必须在 ③ 之前**：否则「过期」与「签名错」可区分，
攻击者能探测「这个 key 是否存在」（D322）。

⚠️ **`object_key` 在签名覆盖范围内** → 不能拿 A 的令牌下 B 的文件（AC-S-176 步骤 2）。

### 4.2 分片上传（D323）

```
① POST /uploads            → 建会话，返回 uploadId
② PUT /uploads/{id}/parts/{n}  ← 幂等：同名文件覆盖
③ POST /uploads/{id}/complete
     a. 列 .uploads/<id>/ 目录 → 检查 1..=total_parts 都存在
        缺失 → 400 STORAGE_PART_MISSING（params 里列出缺的片号）
     b. 按片号顺序拼接 → 算 sha256
        与 declared_checksum 不符 → 409 STORAGE_CHECKSUM_MISMATCH（**不落库**）
     c. 落 object + 删临时目录 + 写 object 元数据
```

⚠️ **拼接必须按片号顺序**，不是按文件系统返回顺序（否则内容错乱且 sha256 会抓不到——
如果 checksum 也是错序算的话。所以 checksum 必须在**拼接后**算）。

### 4.3 异步导入（D324 / D325）

```rust
// worker.rs
pub async fn tick(service, ctx, batch_jobs) -> Result<TickStats> {
    let jobs = store.claim_jobs(ctx, batch_jobs).await?;   // FOR UPDATE SKIP LOCKED
    for job in jobs {
        let mut reader = csv::Reader::new(...);            // **流式**，不读全文件
        let mut batch = Vec::with_capacity(import_batch_size);
        for (line_no, row) in reader.rows().enumerate() {
            match validate(row) {
                Ok(record) => batch.push(record),
                Err(e) => { store.insert_job_error(job_id, line_no, e).await?; failed += 1; }
            }
            if batch.len() >= import_batch_size {
                store.insert_batch(ctx, &batch).await?;    // **一批一事务**
                batch.clear();
                store.update_progress(job_id, processed).await?;  // **每批更新进度**
            }
        }
        // 尾批
    }
}
```

⚠️ **进度必须每批更新**：只在最后写一次 → 「进度可查」是假的（AC-S-180 的判据会红）。

### 4.4 CSV 解析（D320，自己写）

```
状态机：FieldStart → InUnquoted → InQuoted → QuoteInQuoted
规则（RFC 4180）：
  - 字段含 , / " / 换行 → 必须加引号
  - 引号内的 " → 写成 ""（转义）
  - 首行若以 EF BB BF 开头 → 剥掉 BOM
  - CRLF / LF 都是行分隔
  - 单行超过 maxRowBytes → 报错（带行号）
```

⚠️ **引号未闭合**：读到文件末尾仍未闭合 → **报错并指出起始行号**，
**不得**把整个剩余文件当成一个字段（那会让 64 KiB 的行上限失效 → 内存爆）。

---

## 5. 错误码（**8 个**）

逐条见 [acceptance.md](acceptance.md) §全局约定 —— 每个都指到了具体 AC 步骤。

⚠️ `TITLES`：**107 → 115**。
⚠️ 4 个 `storage.*` 事件码**不进 `TITLES`**（D223）。

### 5.1 新增 8 个码必须同步改的**四处**（Dev 最容易漏）

| # | 位置 | 不改的后果 |
|---|---|---|
| 1 | `crates/platform-core/src/titles.rs` 的 `TITLES` 加 8 行 | `titles_cover_every_code` 红（取到兜底） |
| 2 | `crates/platform-server/tests/i18n_titles.rs:179` 加 `const REQ_018_NEW_CODES: usize = 8;` | 该测试硬编码 107；实际 115 → **红** |
| 3 | 同文件 `:59` 的 `all_codes()` 加 `platform_storage::error::ALL_CODES` | ⚠️ **会假绿**：新码不在清单里 → 检查不到（表里缺了也不报） |
| 4 | `packages/i18n/src/locales/{zh-CN,en-US}.json` 各加 8 条 + `packages/i18n/test/i18n.test.ts` 的 codes 数组加 8 个 | 两处都会红 |

> ⚠️ **第 3 条最危险**：唯一一个「不写不会红、但会让守卫失效」的改动
> （REQ-015 的 D318 / S-11 同款）。判据必须是**读源码 grep**，不是「看测试绿」。

---

## 6. 权限点与 manifest

```json
{
  "id": "storage",
  "name": "文件存储",
  "version": "0.1.0",
  "enabled": true,
  "permissions": [
    { "code": "storage:file:read",  "name": "查看与下载文件", "type": "menu" },
    { "code": "storage:file:write", "name": "上传与删除文件", "type": "button" },
    { "code": "storage:job:read",   "name": "查看导入导出任务", "type": "menu" }
  ],
  "menus": [
    { "label": "文件", "icon": "folder", "route": "/storage" }
  ],
  "tenantScoped": {
    "tables": [
      "platform_storage_object",
      "platform_storage_upload",
      "platform_storage_job",
      "platform_storage_job_error"
    ],
    "column": "tenant_id"
  },
  "dependencies": [],
  "config": { /* 6 个键，见下 */ }
}
```

⚠️ 6 个配置项**全部** `"scope": "platform"` + 是 `ConfigSpec` **对象**
+ 键名 lowerCamelCase（段内带 `_` → 服务起不来）。

`permissions.json` 的 `total`：**38 → 41**（实测基线 38）。
⚠️ `capabilities/` 文件数**不增不减**：**14**。

---

## 7. 迁移（3 个 + down）

| 文件 | 内容 |
|---|---|
| `20261122000100_storage_schema.{up,down}.sql` | 4 张表 + 索引 + `base_platform_storage` 授权 |
| `20261122000200_storage_permission_seed.{up,down}.sql` | 3 个权限点 + 1 个菜单 |
| `20261122000300_storage_config_seed.{up,down}.sql` | 6 个配置项 |

⚠️ 命名沿用 `2026112200xxxx` 序列（现有最后一个是 `20261121000600_notify_config_seed`）。

---

## 8. 前端交付

| 文件 | 内容 |
|---|---|
| `apps/shell/src/lib/storage-api.ts` | 封装（列表 / 上传 / 预签名 / 任务 / 错误回执） |
| `apps/shell/src/lib/storage-api.test.ts` | 单测（camelCase 字段、形状容错、CSV 相关纯函数） |

⚠️ **不做页面**（模板管理页 / 任务页）——与 REQ-015 的 TD-056 同一个原因
（TD-048 的壳层限制：`RouteDefinition` 无组件槽 + `App.svelte` 硬编码）。
→ 记 **TD-063**。

---

## 9. 测试落点

| AC | 类型 | 落点 |
|---|---|---|
| 026 / 176 | 单测（**纯函数**，无库无网） | `src/object_store/presign.rs` 内联 |
| 175 | 集成（**真临时目录**） | `tests/storage_local_fs.rs` |
| 177 | 集成（真库 + 真目录） | `tests/storage_multipart.rs` |
| 178 | 集成 + 单测（钩子） | `tests/storage_upload_limits.rs` |
| 179 | 单测（**纯函数**，CSV 状态机） | `src/csv.rs` 内联 |
| 180 / 181 | 集成（真库） | `tests/storage_import.rs` |
| 182 | 集成（真库 + round-trip） | `tests/storage_export.rs` |
| 183 | 集成（真库 + 应用角色） | `tests/storage_rls.rs` |
| 184 | 契约 + 集成 | `tests/contract_c5.rs` + `tests/storage_audit.rs` |
| S3 签名 | 单测（AWS 官方向量） | `src/object_store/s3.rs` 内联 |

**E2E**：无（没有页面 → 无 E2E，与 TD-063 一致）。

---

## 10. ⚠️ 命名禁区（C-5，D216）

`crates/platform-contracts/src/authz_scan.rs:48` 的禁区表含 **`authorize`** 等 8 个别名。
**在 `crates/platform-storage/src/` 里不得出现**。

⚠️ 存储域里极易踩的同义命名：`authorize_download` / `authorize_key_access`
（「是否允许访问这个 key」听起来像鉴权 —— 它**不是**，那是**业务规则**
「这个 key 属不属于本租户」）→ 用 `is_key_visible_to_tenant`。

---

## 11. ⚠️ 已识别的风险

| # | 风险 | 缓解 |
|---|---|---|
| 1 | **S3 只做协议**，没真连过 | TD-059 明码；报告里只写「SigV4 按官方向量验证通过」 |
| 2 | **CSV 自己写**，边界多（BOM/引号/CRLF/未闭合） | 状态机单测逐条覆盖；未闭合必须报错不吞文件 |
| 3 | **5 万行导入**可能内存爆 | 流式 + 单行上限 + 每批一事务（D325） |
| 4 | **multipart 是第二次依赖破例** | 净增 2 包已实测；在 Cargo.toml 注释里指向 D321 |
| 5 | **路径穿越**（`../`） | 落盘前规范化 + 断言哨兵文件不被读到（AC-S-175 步骤 7） |
| 6 | 预签名**时序侧信道** | 用 `subtle::ConstantTimeEq`；**无法功能测试** → TD-062 |
| 7 | 图片处理/Excel 未做 | TD-060 / TD-061 明码 |
