# 决策记录：文件存储与导入导出（REQ-018）

- **需求编号**: REQ-018
- **父需求**: REQ-001
- **状态**: rev 1（2026-11-22 —— 补齐四件套：1 条空泛 AC → **10 条**）
- **基线代码**: `develop` @ `c27528a`（REQ-015 已合并）
- **依赖**: REQ-002 ✅（数据访问层）

---

## 0. 需求来源披露（D122 / dev-guide §9.1）

**REQ-018 的需求来源是 BA 补的，不是用户原话。**

用户 2026-11-17 的 9 项需求清单里**没有「文件存储与导入导出」**。
`roadmap.md:77` 把它标为「⚠️ **BA 补**（用户已裁定继续）」——
用户此后裁定「出了工作流，其他的继续做」→ 本需求继续。

父需求 `REQ-001` 的 **AC-025 / AC-026** 是它的来源（也是 BA 补的 `【补】` 条目）。
→ **本需求的具体形态（10 条 AC / 存储抽象 / 预签名 / 分片 / 异步导入）全部是 BA 的设计**，
每条都要有决策依据。

---

## D319: 存储抽象 = **trait + 2 个实现**（本地 FS 真实现 / S3 做协议不做真连）

### 实测发现（`develop` @ `c27528a`）

| 需要的东西 | 树里有吗 | 结论 |
|---|---|---|
| S3 SDK（`aws-sdk-s3` / `rust-s3` / `object_store`） | ❌ **0 个** | 自己写 |
| SigV4 签名所需的 `hmac` + `sha2` | ✅ `hmac 0.12.1` / `sha2 0.10.9` **已在 `Cargo.lock`**（由 `jsonwebtoken` / `totp-rs` 传递引入）；`sha2` 还是 workspace 直接依赖（`Cargo.toml:88`） | **可自己实现签名** |
| MinIO / 本地对象存储环境 | ❌ **无**（`podman ps` 只有 `base-postgres` + `base-redis`） | S3 无法真连测试 |

### 决定

```rust
pub trait ObjectStore: Send + Sync {
    fn put(&self, key: &str, bytes: Bytes) -> impl Future<Output = Result<()>> + Send;
    fn get(&self, key: &str) -> impl Future<Output = Result<Bytes>> + Send;
    fn delete(&self, key: &str) -> impl Future<Output = Result<()>> + Send;
    fn presign_get(&self, key: &str, ttl: Duration) -> Result<PresignedUrl>;
    // 分片
    fn create_multipart(&self, key: &str) -> impl Future<Output = Result<UploadId>> + Send;
    fn upload_part(&self, key: &str, id: &UploadId, n: u32, bytes: Bytes) -> ...;
    fn complete_multipart(&self, key: &str, id: &UploadId, parts: &[PartETag]) -> ...;
    fn abort_multipart(&self, key: &str, id: &UploadId) -> ...;
}
```

| 实现 | 状态 | 怎么验 |
|---|---|---|
| **`LocalFsStore`** | ✅ **真实现**（落盘到 `BASE_STORAGE_ROOT`） | **真跑**：分片写临时文件 → 合并 → 读回校验 sha256；断点续传（重传已有分片跳过） |
| **`S3Store`** | ⚠️ **只做协议**（SigV4 预签名 + 分片 REST 调用） | **只能单测签名正确性**（用 AWS 官方测试向量），**不能真连** |

### ⚠️ 一条必须遵守的规则

> **S3 的预签名「签名对」≠「能用」。**
> 单测能证明 SigV4 的规范化请求 + HMAC 链正确（有 AWS 官方测试向量），
> 但**没有真实 S3 端点**，无法证明：
> ① 我们的 REST 调用格式被 S3 接受；② 分片合并的顺序与 ETag 处理正确。
> → **必须记 TD**，不许在报告里写「S3 可用」，只能写「SigV4 签名按官方向量验证通过」。

### 代价（记 TD-059）

父 AC-025 写「对象存储为 S3 兼容」——本轮**只达成「协议实现 + 签名验证」，未达成「真连可用」**。

---

## D320: **只做 CSV，Excel 变成显式错误码**（用户裁定）

### 实测发现

| 需要的东西 | 树里有吗 |
|---|---|
| `zip` / `flate2` / `miniz_oxide` | ❌ **全部 0 个** |
| `quick-xml` / `xml-rs` / `roxmltree` | ❌ **全部 0 个** |

`.xlsx` 的本质是 **ZIP 容器 + 若干 XML 部件**（`xl/worksheets/sheet1.xml` /
`xl/sharedStrings.xml` / `[Content_Types].xml` …）→ **零依赖下做不出真 xlsx**。

### 决定

1. **CSV 做真实现**：自己写 RFC 4180 的解析与生成（≈150 行）；
2. **Excel 不做**，但**不是静默失败**：
   上传 `.xlsx` / `.xls` → **明确错误码 `STORAGE_UNSUPPORTED_FORMAT`**，
   `detail` 里写「only csv is supported in this release」；
3. **模板下载也只提供 CSV**（`GET /api/storage/imports/template` 返回 `text/csv`）。

### 为什么不做「假 xlsx」

「把 CSV 改个扩展名叫 `.xlsx`」= **最坏的假绿**：Excel 打不开，用户以为文件坏了，
而我们所有测试都会绿（因为我们只测字节）。与 REQ-022 修复轮抓到的「判据恒真」同类。

### 代价（记 TD-060）

`demand.md` 的「Excel / CSV 模板下载」本轮**只达成 CSV**。

---

## D321: 上传传输 = **启用 axum `multipart` feature**（用户裁定，**第二次依赖破例**）

### 实测发现

`axum 0.8.9` 的 `multipart` feature = `["dep:multer"]`，`multer = "3.0.0"`（optional）。

**`multer 3.0` 的传递依赖实测**（`crates.io` API）：

| 依赖 | 在 `Cargo.lock` 里吗 |
|---|---|
| `multer` | ❌ **需新增** |
| `encoding_rs` | ❌ **需新增** |
| `httparse` / `log` / `mime` / `spin` / `memchr` / `futures-util` | ✅ 全部已在 |

> ⚠️ **rev 2 修正（实测打脸）**：rev 1 由上面这张表得出「净新增只有 **2** 个包」，
> 这是**只看了一层传递依赖**的错误。实现完成后对 `Cargo.lock` 做实测对比
> （`develop` 303 包 → `feature/REQ-018` 309 包，用 `name = "..."` 正则求差集）：

| 新增包 | 版本 | 由谁引入 |
|---|---|---|
| `multer` | 3.1.0 | `axum`（`multipart` feature） |
| `encoding_rs` | 0.8.44 | `multer` |
| `core_detect` | 1.0.0 | `encoding_rs` |
| `multiversion_no_op` | 1.0.0（proc-macro） | `encoding_rs` |
| `simdutf8` | 0.1.5 | `encoding_rs` |
| `platform-storage` | 0.1.0 | **本需求自己的新 crate**（不算第三方） |

→ **第三方包净新增 = 5 个**（`multer` + `encoding_rs` + `core_detect` + `multiversion_no_op` + `simdutf8`），
**不是 2 个**。`cargo tree -p encoding_rs -e normal` 确认后 3 个是**实际编译依赖**（不是可选 feature）。
删除包：**0 个**。

### 决定

**启用 `axum` 的 `multipart` feature。** 这是继 **D123（TLS 栈）** 之后
**第二次依赖预算破例**，理由：

1. 父 AC-025 要求「上传大文件（分片）」——multipart 是**标准做法**，
   自定义协议会让前端与第三方客户端都要特殊适配；
2. **第三方净新增 5 个包**（rev 2 实测值；rev 1 曾误记为 2 个）——
   仍然不是「一个新的生态」（`aws-sdk-s3` 那种才是），5 个都是小工具包：
   `core_detect` / `multiversion_no_op` 是 CPU 特性探测，`simdutf8` 是 SIMD UTF-8 校验；
3. 自己实现 multipart 解析（边界扫描 + 流式 body）约 300 行且**比 CSV 解析更容易出安全洞**
   （边界冲突、头部注入、`Content-Length` 欺骗）。

> ⚠️ **破例的边界必须重新明确**：破例范围是「multipart 这一条链路」，
> **不**包含 `encoding_rs` 继续往下拉任何包之外的东西。若后续 `encoding_rs` 升级带进更多包，
> 需重新走裁决（不是自动放行）。

### 同时必须做的事

- `Cargo.toml` 里 `axum` 要显式写 `features = ["multipart"]`（不能靠默认）；
- 在 `Cargo.toml` 的注释里写明这是**第二次破例**并指向 D321（与 D123 的写法一致）；
- 上传大小上限走配置（`storage.maxUploadBytes`，默认 100 MiB）——
  **不能**让一个请求把内存吃光。

---

## D322: 预签名 = **HMAC-SHA256 + 显式 TTL**，**不引入 JWT**

### 决定

本地 FS 的「预签名」用 **HMAC-SHA256 签名令牌**（不是 JWT）：

```
token = base64url( key_id || expires_at_unix || hmac_sha256(secret, key_id|expires_at|object_key) )
GET /api/storage/download/{token}
```

- **`expires_at` 在签名覆盖范围内** → 改时间就签名失效；
- **`object_key` 也在签名覆盖范围内** → 不能拿 A 的令牌下 B 的文件；
- 校验顺序：① 签名（用 `constant_time_eq` 防时序侧信道）→ ② 过期 → ③ 取文件。
  ⚠️ **签名先于过期**：否则攻击者能通过「过期报错 vs 签名错报错」探测 key 是否存在。

### 为什么不复用 IAM 的 JWT

1. JWT 是**用户身份**令牌，预签名 URL 是**一次性资源访问**凭证 —— 语义不同；
2. JWT 的密钥是 ES256 登录密钥，混用会让「登录密钥泄露」的爆炸半径扩大到所有文件；
3. `jsonwebtoken` 已可用，但**用 HMAC 更简单且不需要引入新的密钥轮换逻辑**。

### ⚠️ `subtle` 从哪来（S-08）

常数时间比较用 `subtle::ConstantTimeEq`。
`subtle` **已在 `Cargo.lock`**（由 `ring` / `rustls` 传递引入）→
加 workspace 条目**不新增 lock 条目**（与 `hmac` 同款，D329）。

⚠️ **为什么必须是常数时间**：用 `==` 比签名字符串时，比较会在**第一个不同的字节**提前返回
→ 攻击者能通过测响应时间逐字节猜出正确签名（时序侧信道）。
⚠️ 但**「用了常数时间比较」这件事无法用功能测试断言**（`==` 与 `constant_time_eq`
功能等价，差别只在耗时）→ 记 **TD-062**，靠代码评审保证。

---

## D323: 分片上传 = **服务端会话表 + 幂等 part**，断点续传靠 **part 已存在则跳过**

### 决定

`platform_storage_upload`（上传会话表）：

| 列 | 说明 |
|---|---|
| `upload_id` / `tenant_id` / `object_key` | |
| `total_parts` / `part_size` | 分片数 / 每片大小 |
| `status` | `pending` / `completing` / `completed` / `aborted` |
| `created_at` / `updated_at` / `expires_at` | 未完成的会话超时清理 |

- **`part_number` 幂等**：同一 `(upload_id, part_number)` 重复上传 → **覆盖**（不报错）；
  这是断点续传的基础：客户端不知道哪片传成功了，重传一遍不会坏；
- **`complete` 时校验**：所有 `1..=total_parts` 都已上传，缺失 → **400 并列出缺失的片号**；
- **合并后校验 sha256**：与客户端声明的 `checksum` 比对，不一致 → `STORAGE_CHECKSUM_MISMATCH`
  （**不能**把坏文件当成功落库）。

---

## D324: 异步导入导出 = **同 REQ-015 的数据库 worker 模式**（复用已验证的形态）

### 决定

**不发明新机制** —— 直接沿用 D296 的形态：

| 项 | 做法 |
|---|---|
| 队列表 | `platform_storage_job`（`kind = import/export`，`status`，`processed_rows`，`error_rows`，`next_attempt_at`） |
| 出队 | `SELECT ... FOR UPDATE SKIP LOCKED` |
| 入口 | `xtask storage worker`（与 `xtask notify worker` 同款） |
| 进度 | `processed_rows` / `total_rows` **每批更新**（不是最后才写） |
| 错误回执 | `platform_storage_job_error`（`row_number` / `column` / `reason`），**带行号** |

### 为什么复用而不是新做

REQ-015 的 worker 形态已经过 **7 个真库测试**验证（含 4 worker 并发 100 条不重不漏）。
重新设计一套只会引入**同一类 bug**（重复投递 / 崩溃残留 / 并发状态机）。

---

## D325: 5 万行导入 = **流式分批 + 每批独立事务**

### 决定

- **不把 5 万行读进内存**：`csv` 解析器**逐行**产出，攒够 `batch_size`（默认 1000）就提交一批；
- **每批独立事务** → 前面 4 万行成功、第 4 万零 1 行失败，**前 4 万行仍然入库**
  （父 AC-026 要求「合法行入库，非法行返回回执」—— 就是**部分成功**语义）；
- **非法行不回滚整批**：解析失败的行记进 `job_error` 表并**跳过**，继续下一行；
- ⚠️ **单行长度上限**（默认 64 KiB）：防一个畸形行把内存吃光（CSV 的「引号未闭合」能让
  解析器一直读到文件末尾）。

---

## D326: 病毒扫描 = **钩子 trait，本轮不做真实现**

### 决定

```rust
pub trait VirusScanner: Send + Sync {
    fn scan(&self, bytes: &[u8]) -> impl Future<Output = Result<ScanVerdict>> + Send;
}
pub enum ScanVerdict { Clean, Infected { signature: String }, Skipped }
```

- 默认实现 [`NoopScanner`] 返回 `Skipped`（**明确表示「没扫」**，不是 `Clean`）；
- **落库时记录 `scan_verdict`**：`skipped` / `clean` / `infected`；
- `infected` → **拒绝落库** + 删除临时文件 + `STORAGE_FILE_INFECTED`；
- **真实的 ClamAV 接入记 TD**（需要 `clamd` 客户端 + 一个 clamd 实例）。

### ⚠️ 为什么不默认 `Clean`

「没扫」和「扫过且干净」是**两件事**。
默认返回 `Clean` 会让「病毒扫描已启用」变成一句谎话（D292 的「未配置 ≠ 可用」同款）。
→ 默认 `Skipped`，并在**响应体里如实返回**，让调用方知道这个文件没被扫过。

---

## D327: 图片处理 = **不做**，记 TD

### 理由

缩略图 / 尺寸裁剪需要图像解码库（`image` crate 或 `libvips` 绑定），
**零依赖下做不出**；手写 JPEG/PNG 解码器是**数万行**的事，且是安全高风险面
（历史上 libjpeg/libpng 的洞最多）。

→ `demand.md` 的「图片处理」本轮**不做**，记 **TD-061**。

---

## D328: AC 编号 **AC-S-175 ~ AC-S-184**（+ 既有 AC-S-026 重写）

实测：全仓最大 AC-S 编号 = **174**（REQ-015 合并后）；`175`~`184` 未被占用。

| AC | 主题 |
|---|---|
| **AC-S-026** | （既有，**重写**）预签名链接限时，过期被拒 |
| **AC-S-175** | 存储抽象的 trait + 2 实现 + 本地 FS 真跑了分片 |
| **AC-S-176** | 预签名令牌的**篡改防护**（改时间 / 改 key 都失效） |
| **AC-S-177** | 分片上传：幂等 part + 缺片检测 + 合并后 sha256 校验 |
| **AC-S-178** | 上传大小上限 + 病毒扫描钩子（`Skipped` 如实返回） |
| **AC-S-179** | CSV 解析（RFC 4180：引号 / 转义 / 换行 / BOM）+ Excel 显式拒绝 |
| **AC-S-180** | 异步导入：5 万行分批 + 进度可查 + 部分成功 |
| **AC-S-181** | 导入错误回执：**带行号与列名**，非法行跳过不入库 |
| **AC-S-182** | 导出：CSV 生成 + 异步 + 可下载 |
| **AC-S-183** | 任务表租户隔离 + RLS（3 张表） |
| **AC-S-184** | 权限点三处齐 + C-5 + 启动对账 + 审计埋点 |

**共 11 条**（10 新 + 1 既有重写）。

---

## D329: 新 crate `platform-storage`

```
crates/platform-storage/
  src/
    lib.rs
    error.rs        // 9 个错误码
    events.rs       // 审计事件码
    model.rs        // 表行 + DTO
    object_store/   // D319 的 trait + 两个实现
      mod.rs
      local_fs.rs   // ✅ 真实现
      s3.rs         // ⚠️ 只做协议（D319）
      presign.rs    // HMAC 令牌（D322）
    csv.rs          // RFC 4180 解析 + 生成（D320）
    scanner.rs      // 病毒扫描钩子（D326）
    store.rs        // 数据访问
    service.rs      // 编排（上传会话 / 导入导出任务）
    worker.rs       // tick()（D324）
    http.rs         // 平台端点
    authz.rs        // 3 个 require_permission
    config.rs       // 配置源
```

依赖：`platform-core` + `platform-db` + `platform-config` + `axum`（含 **multipart**，D321）
+ `hmac` / `sha2` / `base64` / `bytes`。

⚠️ **`hmac` 需要加 workspace 条目**：它现在只在 `Cargo.lock`（由 `jsonwebtoken` /
`totp-rs` 传递引入），**没有 workspace 直接依赖**。
→ 加一行 `hmac = "0.12"`（**不新增 lock 条目**，包已在 lock 里）。

---

## D330: **9 个错误码**（每个都必须有 AC 会触发）

| 码 | HTTP | 何时 | 触发它的 AC |
|---|---|---|---|
| `STORAGE_OBJECT_NOT_FOUND` | 404 | 对象不存在 | 175 步骤 5 |
| `STORAGE_PRESIGN_EXPIRED` | 403 | 预签名过期 | 026 步骤 3 / 176 |
| `STORAGE_PRESIGN_INVALID` | 403 | 预签名被篡改（签名不符） | 176 步骤 2/3 |
| `STORAGE_PART_MISSING` | 400 | complete 时缺片 | 177 步骤 4 |
| `STORAGE_CHECKSUM_MISMATCH` | 409 | 合并后 sha256 不符 | 177 步骤 5 |
| `STORAGE_UPLOAD_TOO_LARGE` | 413 | 超过 `maxUploadBytes` | 178 步骤 2 |
| `STORAGE_UNSUPPORTED_FORMAT` | 415 | 上传 `.xlsx` 等（D320） | 179 步骤 5 |
| `STORAGE_FILE_INFECTED` | 422 | 扫描判定为感染 | 178 步骤 4 |
| **`STORAGE_TTL_OUT_OF_RANGE`** | 400 | 请求的 TTL 超上限或为 0 | 026 步骤 5 |

⚠️ **`TITLES` 行数**：**107 → 116**（+9）。
⚠️ 事件码（**4 个** `storage.*`）**不进** `TITLES`（D223 同款）。

---

## D331: **6 个配置项**（每个必须带 `"scope": "platform"` + 是 `ConfigSpec` 对象）

| 键 | 类型 | 默认 | 说明 |
|---|---|---|---|
| `storage.maxUploadBytes` | `number` | `104857600`（100 MiB） | 单文件上限 |
| `storage.partSizeBytes` | `number` | `8388608`（8 MiB） | 分片大小 |
| `storage.presignTtlSeconds` | `number` | `900`（15 分钟） | 预签名有效期 |
| `storage.importBatchSize` | `number` | `1000` | 导入每批行数 |
| `storage.maxRowBytes` | `number` | `65536` | 单行上限（D325） |
| `storage.uploadSessionTtlSeconds` | `number` | `86400` | 未完成上传会话的超时 |

⚠️ 键名必须 **lowerCamelCase**（契约 C9：段内带 `_` 会让服务起不来）。
⚠️ 每个键必须带 `"scope": "platform"`（REQ-016 的 S-01 教训）。
⚠️ `config` 段必须是 `ConfigSpec` 对象，**不是裸值**（S-02 教训）。

---

## D332: 权限点 **3 个**

| 权限点 | 用途 |
|---|---|
| `storage:file:read` | 查看 / 下载文件 + 导出结果 |
| `storage:file:write` | 上传 / 删除文件 + 建上传会话 |
| `storage:job:read` | 查看导入导出任务与错误回执 |

- **C-5**：3 个各要有字面量 `require_permission("<字面量>")`，在 `crates/platform-storage/src/` 内；
- ⚠️ 命名禁区（D216）：`authorize` 等 8 个别名**不得**出现 ——
  存储域里「是否允许访问这个 key」听起来像鉴权，用 `is_key_visible_to_tenant` 这类名字。

---

## D333: 导入目标实体 = **新增第 5 张租户表 `platform_storage_probe_row`**（用户 2026-11-22 裁定）

### 问题（实现复核 F-01 / F-02 发现）

`worker.rs` 处理合法行的分支是 `Ok(()) => {}` —— **空操作**。
全仓没有任何「`entity` → 目标表」的写入路径，且 `probe_rows` 这张表
在 `migrations/` 里**根本不存在**。

→ AC-S-180 步骤 5（目标表恰好 49990 行）与 AC-S-181 步骤 4（只有 3 行入库）
**两条判据都没有实现**；而 AC-S-180 的测试把 5 万行换成 2500 行、
并**删掉对目标表的全部断言** → 测试因此全绿。

### 规格自身的矛盾

| AC | 前置条件写的列 |
|---|---|
| AC-S-180 | `objectKey,sizeBytes` |
| AC-S-181 | `email` / `amount`（且明确要求「列名必须是真实的列名」） |

同一个 `entity` 两套列 → 无论怎么实现都满足不了两条。

### 决定（用户裁定：方案 A）

1. **新增 `platform_storage_probe_row` 表**（迁移 `20261122000400`，**租户级 + RLS**）；
   列固定为 `id` / `tenant_id` / `job_id` / `email` / `amount` / `note` / `created_at`；
2. **列名以 AC-S-181 为准**（`email` / `amount`）—— 因为 AC-S-181 明确禁止下标式列名，
   而 AC-S-180 的表头只是「举例」；
3. **AC-S-180 的表头同步改为 `email,amount,note`**（rev 2）；
4. **AC-S-183 从 4 张表改为 5 张表**（rev 2）：
   `tenantScoped.tables` 双向相等断言、直连 RLS 验证都要覆盖新表。

> ⚠️ 第 4 条**不是放宽判据，是收紧**：新增的表存的是**业务数据**而非元数据，
> 漏隔离的后果比原来更严重（A 租户导入的数据会出现在 B 租户的导出里）。

### 为什么是「通用探针表」而不是业务表

本需求做的是**通道能力**（CSV → 解析 → 校验 → 落库），
具体导到哪张业务表属于各业务域的事。本轮提供列固定的探针实体 `probe_row`：
① 证明链路真通；② 给 AC-S-180/181 一个可断言的落点。
真实业务表的接入是后续需求的事（记 **TD-064**）。

---

## D334: 审计事件 = **真实现**（注入 `AuditSink`，四处埋点，用户 2026-11-22 裁定）

### 问题（实现复核 F-03 发现）

4 个事件码 `storage.file.uploaded` / `storage.file.deleted` /
`storage.import.completed` / `storage.export.completed` 在 `events.rs` 里
**只有常量定义**，全仓零发出点；design §9 点名的 `tests/storage_audit.rs` 不存在
→ AC-S-184 步骤 5 整条未实现。

### 决定

1. `StorageService` 增加 `audit: Arc<dyn AuditSink>` 字段
   （复用 `platform_core::extension::audit::{AuditEvent, AuditSink}`，
   **与 `platform-app` / `platform-oauth` 同一套机制**，不自创）；
2. **四处埋点**：`put_object` 成功 → `file.uploaded`；
   `delete_object` 成功 → `file.deleted`；
   导入任务转 `completed` → `import.completed`；
   导出任务转 `completed` → `export.completed`；
3. 审计失败**不影响主流程**（与 `platform-app:660` 同款：`tracing::warn!` 后继续）——
   审计是旁路，不能因为审计写不进去就让用户的上传失败；
4. 新增 `tests/storage_audit.rs`：真库 + 走完「上传 → 删除 → 导入 → 导出」，
   断言 `platform_audit_log` 里**恰好 4 条**（四个事件码各一条），
   且 `ChainVerifier::verify(tenant-a)` → `ok == true`。

---

## D335: `storage.partSizeBytes` 必须**被真实读取**（实现复核 F-04）

### 问题

`StorageSettings.part_size_bytes` 只被 `config.rs` 填充、被单测断言默认值，
**业务代码零读取** —— `create_upload` 用的是客户端传入的 `body.part_size`。
改这个配置项对行为无影响 → 违反 D331「6 个配置项每个必须被真实读取」。

### 决定

`create_upload` 增加服务端侧约束：**`body.part_size` 不得大于 `settings.part_size_bytes`**
（即配置项是「单片上限」而不是「建议值」）。超出 → `STORAGE_UPLOAD_TOO_LARGE`
（带 `maxBytes`）。

理由：`part_size` 完全由客户端决定时，一个恶意客户端可以声明
`totalParts = 1000000, partSize = 1`（差一字节一片）→ 在磁盘上制造上百万个小文件
（inode 耗尽），或者反过来声明超大单片绕过 `maxUploadBytes` 的**逐片**检查。
服务端必须有上限。

> ⚠️ 语义明确：`partSizeBytes` = **单片最大字节数**（不是「推荐值」）。
> 这条要写进配置项的 `description`，否则下一个维护者还会以为是建议值。

---

## D336: 畸形 CSV 的错误暴露时机 = **异步**（202 → 任务 failed），不是同步 400（用户 2026-11-22 裁定）

### 问题（TD-068）

AC-S-179 步骤 4/5 原文要求「引号未闭合 / 单行超限 → **400**，错误指出**行号**」。
但实现里这两类错误在**异步 worker** 的 `csv::parse` 里产生 →
HTTP 层只有 202，用户拿到的是「任务 failed + error 含行号」。

### 为什么不能改成同步 400

`create_import` 若要在请求线程里返回 400，必须**同步解析整个文件**：

1. 文件是**先从对象存储读**的（可能几百 MB）→ 同步读会让请求超时；
2. **D325 要求流式 + 分批 + 每批独立事务**，同步全量解析与之直接矛盾
   （不是「实现没做」，是两个决定互斥）；
3. 只预检文件**头部**（前 N 行）**抓不到**「引号未闭合」——
   这个错误恰恰常在文件末尾，只查头部会给调用方一个
   **「看起来没问题」的假安全**。这比不做预检更糟。

### 决定（用户裁定：方案 1，改规格）

1. **AC-S-179 步骤 4/5 改为**：`202 ACCEPTED` → worker 把任务置
   `status == "failed"` → `job.error` **含行号**（步骤 5 还要含上限）；
2. 判据**仍然可断言**：断言 `status == "failed"` **且** `error` 含 `line N`
   —— 不是「随便怎样都行」的松判据；
3. TD-068 关闭。

### 为什么不降低信息量

| | 原判据（400） | 新判据（202 + failed） |
|---|---|---|
| 能否知道哪一行错 | 能 | 能（`error` 里有 `line N`） |
| 能否知道为什么错 | 能 | 能（`error` 里有原因） |
| 何时知道 | 提交时（同步） | 任务跑完后（异步） |
| 超时风险 | **有**（大文件同步读） | 无 |
| 与 D325 冲突 | **是** | 否 |

→ 给用户的**可定位信息完全相同**，只是**暴露时机**从同步变成异步。
这是异步导入的**本质语义**，不是妥协。

---

## 未决 / 待用户复核

| # | 事项 | 结论 |
|---|---|---|
| 1 | S3 **只做协议、不真连** | ✅ 用户 2026-11-22 裁定 → TD-059 |
| 2 | **只做 CSV**，Excel 显式拒绝 | ✅ 用户 2026-11-22 裁定 → TD-060 |
| 3 | **启用 axum multipart**（第二次依赖破例，净增 **5** 包 —— rev 2 实测修正） | ✅ 用户 2026-11-22 裁定 |
| 4 | 病毒扫描只做钩子（默认 `Skipped`） | BA 提议（D326） |
| 5 | 图片处理不做 | BA 提议（D327）→ TD-061 |
| 6 | 异步导入复用 REQ-015 的 worker 形态 | BA 提议（D324） |
| 7 | **导入目标表 = 新增第 5 张租户表 `probe_row`** | ✅ 用户 2026-11-22 裁定 → D333 |
| 8 | **审计事件真实现**（注入 `AuditSink` + 真库测试） | ✅ 用户 2026-11-22 裁定 → D334 |
| 9 | **`partSizeBytes` 变成硬上限** | BA 提议（D335） |
| 10 | **畸形 CSV 错误暴露时机 = 异步**（202 + 任务 failed，不是同步 400） | ✅ 用户 2026-11-22 裁定 → D336 |
