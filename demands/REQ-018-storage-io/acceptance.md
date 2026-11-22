# 验收标准：文件存储与导入导出

- **需求编号**: REQ-018
- **父需求**: REQ-001
- **状态**: rev 1（2026-11-22 —— 从 1 条空泛 AC 补齐到 **11 条**）
- **对应验收标准**: **11 条**（AC-S-026 **重写** + **AC-S-175 ~ AC-S-184**）+
  **1 个「明确不做」清单**（AC-S-185.b，**不是 AC**，是防止后人以为漏了的说明）
- **决策依据**: [decisions.md](decisions.md) **rev 1（D319~D332）**
- **基线代码**: `develop` @ `c27528a`（REQ-015 已合并）
- **更新时间**: 2026-11-22

> 父需求的完整验收标准见 [REQ-001 acceptance.md](../REQ-001-platform-foundation/acceptance.md)。
> 本文件只列出本子需求承担的部分。

## 需求来源披露（D122）

**REQ-018 不是用户原话需求**。用户 2026-11-17 的 9 项清单里没有「文件存储」；
`roadmap.md:77` 标「⚠️ **BA 补**」，用户此后裁定继续。
**11 条 AC 的具体形态全部是 BA 的设计**，每条都有 `decisions.md` 的依据。

---

## rev 1 变更摘要（**Dev 必读**）

rev 0 只有 **1 条空泛 AC**（AC-S-026，正文是「行为与本条标题一致」+
「按需求描述构造前置状态」—— **零个可断言事实**），而 `demand.md` 列了 **3 项功能要求**
（对象存储抽象 + 上传下载分片断点 + 图片处理病毒扫描 + Excel/CSV 导入导出）→
**1 条 AC 验收不了这些**。rev 1 补齐到 **11 条**。

**两条与父 AC 的偏差（用户 2026-11-22 已裁定）**：

1. **S3 只做协议、不真连**（父 AC-025 要求「S3 兼容」）→ **TD-059**
   ⚠️ 明码：SigV4 签名按 AWS 官方向量验证通过，但**没有真实端点**证明可用。
2. **只做 CSV，Excel 显式拒绝**（`demand.md` 要求「Excel / CSV」）→ **TD-060**
   ⚠️ 明码：`.xlsx` = ZIP + XML，两个库都不在树里 → 零依赖做不出来。

---

## 全局约定

| 项 | 约定 | 依据 |
|---|---|---|
| 应用连接角色 | **`base_app`**（`base_platform_storage` 是 NOLOGIN 组角色，连不上） | D165 |
| 造数 / 篡改连接 | **`base`**（表 owner + 超管） | — |
| 库 | **`base_test`**（绝不写 `base_dev`） | dev-guide §9.8 |
| `app.tenant_id` | **事务局部**；涉及租户上下文的查询**必须在单个事务内** `set_config` → 查询 | D148 |
| 错误体 | **一律 Problem Details**（RFC 9457，契约 C6） | 契约 C6 |
| 字段名 | **camelCase**（契约 C13） | D167 |
| 时间格式 | UTC、RFC3339、**微秒 6 位** | D142 |
| 静态扫描 | 必须带**阳性对照** | dev-guide §9.2 |
| 判据落点 | 指到 `file:line` 的 `assert!`，**同时给函数名**；AC 字段清单逐个断言 | **§9.15 / §9.15.1 / §9.15.2** |
| AC 里的数字 | 必须有 `file:line` 或实测命令支撑 | **D275** |

### 本需求的依赖破例（**第二次**，D321）

| 破例 | 内容 | 净新增 |
|---|---|---|
| **D123**（第一次） | TLS 栈（`rustls` / `ring` / `hyper-rustls`） | REQ-013 时 |
| **D321**（第二次，本次） | `axum` 的 `multipart` feature → `multer 3.0` | **2 个包**：`multer` + `encoding_rs`（其余 6 个传递依赖已在 lock） |

⚠️ `hmac` **不是新增依赖**：它已在 `Cargo.lock`（由 `jsonwebtoken` / `totp-rs` 传递引入），
本需求只是**加 workspace 条目**把它变成直接依赖（**不新增 lock 条目**）。

### 本需求的平台错误码（**9 个**）

| 码 | HTTP | 何时 | 哪个 AC 触发它 |
|---|---|---|---|
| `STORAGE_OBJECT_NOT_FOUND` | 404 | 对象不存在 | AC-S-175 步骤 5 |
| `STORAGE_PRESIGN_EXPIRED` | 403 | 预签名过期 | AC-S-026 步骤 3 / AC-S-176 步骤 4 |
| `STORAGE_PRESIGN_INVALID` | 403 | 预签名被篡改 | AC-S-176 步骤 2 与 3 |
| `STORAGE_PART_MISSING` | 400 | complete 时缺片 | AC-S-177 步骤 4 |
| `STORAGE_CHECKSUM_MISMATCH` | 409 | 合并后 sha256 不符 | AC-S-177 步骤 5 |
| `STORAGE_UPLOAD_TOO_LARGE` | 413 | 超 `maxUploadBytes` | AC-S-178 步骤 2 |
| `STORAGE_UNSUPPORTED_FORMAT` | 415 | 上传 `.xlsx` 等 | AC-S-179 步骤 5 |
| `STORAGE_FILE_INFECTED` | 422 | 扫描判定为感染 | AC-S-178 步骤 4 |
| `STORAGE_TTL_OUT_OF_RANGE` | 400 | 请求的 TTL 超上限或为 0 | AC-S-026 步骤 5 |

⚠️ **规则**：每个错误码必须有一条 AC 会**真的触发**它，否则删掉（**9 个全部有触发点**）（D226 / REQ-016 删过两个死码）。

⚠️ **为什么要区分 413 与 400**（S-07）：都是「输入有问题」，但语义不同 ——
**413 = 资源超限**（请求本身**合法**，只是太大，RFC 9110 §15.5.14）；
**400 = 格式 / 取值非法**（请求本身**不合法**）。
→ `STORAGE_UPLOAD_TOO_LARGE` 用 413、`STORAGE_UNSUPPORTED_FORMAT` 与
`STORAGE_TTL_OUT_OF_RANGE` 用 400。这是 HTTP 的既有语义，不是我们的发明。
⚠️ `TITLES` 行数：**107 → 116**（+9）。
⚠️ 事件码（**4 个** `storage.*`：`storage.file.uploaded` / `storage.file.deleted` /
`storage.import.completed` / `storage.export.completed`）**不进 `TITLES`**（D223）。

### 本需求的配置项（**6 个**）

| 键 | 类型 | 默认 | scope |
|---|---|---|---|
| `storage.maxUploadBytes` | `number` | `104857600` | **platform** |
| `storage.partSizeBytes` | `number` | `8388608` | **platform** |
| `storage.presignTtlSeconds` | `number` | `900` | **platform** |
| `storage.importBatchSize` | `number` | `1000` | **platform** |
| `storage.maxRowBytes` | `number` | `65536` | **platform** |
| `storage.uploadSessionTtlSeconds` | `number` | `86400` | **platform** |

⚠️ 每个键必须 `"scope": "platform"`（S-01 教训）+ 是 `ConfigSpec` 对象（S-02 教训）
+ 键名 lowerCamelCase（契约 C9：段内带 `_` 会让服务起不来）。

---

## 本需求自有 AC

---

### AC-S-026: 预签名链接限时，过期后被拒

> ⚠️ **rev 1 重写**。原文：「**测试步骤**: 1. 按需求描述构造前置状态 2. 执行该能力对应的操作序列
> **预期结果**: 行为与本条标题一致，且失败路径有明确错误码」——
> **零个可断言事实**（「按需求描述构造」是循环引用）。

- **前置条件**: `tenant-a` 下有一份已上传的文件（`objectKey = "docs/a.txt"`，内容已知）；
  管理员持 `storage:file:read`；`storage.presignTtlSeconds` 为默认 **900**。
- **测试步骤**:
  1. `POST /api/storage/objects/docs%2Fa.txt/presign`（body `{ttlSeconds: 2}`）；
  2. 立刻用返回的 `url` 下载；
  3. 等 **3 秒**后再用同一个 `url` 下载；
  4. 解析 `url` 里的 token，检查是否含明文过期时间；
  5. 用 `ttlSeconds: 86400 * 365`（一年）请求预签名。
- **预期结果**:
  - 步骤 1：**200**，返回 `{ url, expiresAt }`；
    `expiresAt` 是 RFC3339 微秒 6 位，且 `expiresAt - now` 在 **1~2 秒**之间
    （⚠️ **不是**「≤ 2 秒」—— 那对「立即过期」也成立）；
  - 步骤 2：**200**，响应体**逐字节等于**上传时的内容；
  - 步骤 3：**403** + `STORAGE_PRESIGN_EXPIRED`；
  - 步骤 4：token 是 base64url，**解码后包含**过期时间字段
    （⚠️ 不是「不可读的随机串」—— 实现必须能让运维排障时看出什么时候过期）；
  - 步骤 5：**400 + `STORAGE_TTL_OUT_OF_RANGE`**，且**不签发** ——
    TTL 上限 `86400` 秒（超了拒绝，**不是截断**：截断会让调用方以为自己拿到了 1 年的链接）；
    ⚠️ 同时断言 `ttlSeconds: 0` 也返回该码（**边界**：0 是非法而非「立即过期」）。
- **类型**: 安全

---

### AC-S-175: 存储抽象的 trait + **2 个实现**，本地 FS 真跑完分片

- **前置条件**: `BASE_STORAGE_ROOT` 指向一个临时目录；`tenant-a` 存在。
- **测试步骤**（**主路径 + 4 条独立路径**）:
  1. （主）`PUT /api/storage/objects/docs%2Fa.txt`（body 是 1 MiB 二进制）；
  2. `GET /api/storage/objects/docs%2Fa.txt` 读回；
  3. 检查**落盘的文件**（不是走 HTTP，直接读 `BASE_STORAGE_ROOT` 下的文件）；
  4. **路径 2**：`GET` 一个不存在的 key；
  5. **路径 3**：`DELETE` 后再 `GET`；
  6. **路径 4**：同一个 key 再 `PUT` 一次（覆盖）；
  7. **路径 5**：`key` 里带 `../`（`docs/../../etc/passwd`）。
- **预期结果**:
  - 步骤 2：**逐字节等于**步骤 1 上传的内容（用 sha256 比对，不是比长度）；
  - 步骤 3：文件落在 `BASE_STORAGE_ROOT` 下，且**文件名不含** `..`；
  - 步骤 4：**404** + `STORAGE_OBJECT_NOT_FOUND`；
  - 步骤 5：`DELETE` **204**；之后 `GET` **404**；
  - 步骤 6：两次 `PUT` 都 **200**，读回的是**第二次**的内容（覆盖语义）；
  - 步骤 7：**400**，且满足**两条判据**（⚠️ **安全判据**：路径穿越）：
    ① **直接判据**：请求后列出 `BASE_STORAGE_ROOT` 下**所有文件的绝对路径**，
    断言**每一个**都以 `canonicalize(root)` 为前缀（`Path::starts_with`）——
    这约束的是**所有**落盘，不只这一次；
    ② **间接判据（阳性对照）**：在 `BASE_STORAGE_ROOT` 的父目录放一个哨兵文件，
    请求后该文件**内容未变**（证明没被覆盖）；
    ⚠️ 只写 ② 不够：它只证明「这一次没被读到」，不证明「落盘路径一定在 root 之内」；
  - **`S3Store` 的存在性**：`cargo test` 里有 SigV4 签名测试且**通过**
    （⚠️ 只证明签名对，**不证明** S3 可用 —— 见 TD-059）。
- **类型**: 功能 / 安全

---

### AC-S-176: 预签名令牌的**篡改防护**

- **前置条件**: 同 AC-S-026 步骤 1 拿到的 `url`（记为 `U`）。
- **测试步骤**（**4 条独立路径**）:
  1. 把 `U` 的 token 里**过期时间**改大一天（重新 base64url 编码）；
  2. 把 `U` 的 token 里 **object key** 改成另一个文件的 key；
  3. 把 `U` 的 token 的**签名部分**改一个字符；
  4. 用**过期但签名正确**的 token（AC-S-026 步骤 3 的那个）。
- **预期结果**:
  - 步骤 1/2/3：**全部 403 + `STORAGE_PRESIGN_INVALID`**
    （⚠️ **不是** 404、**不是** 200 —— 篡改必须被识别为「签名不符」，
    不能退化成「找不到」让攻击者能探测 key 是否存在）；
  - 步骤 4：**403 + `STORAGE_PRESIGN_EXPIRED`**
    （⚠️ 与步骤 1~3 **错误码不同**：签名对但过期 ≠ 签名错）；
  - **判据独立性**：上面 4 条**互不覆盖** —— 具体地说：
    ① 若实现不把 `expires_at` 纳入签名 → 步骤 1 红；
    ② 若不把 `object_key` 纳入签名 → 步骤 2 红；
    ③ 若用 `==` 比签名字符串（非常数时间）→ 本条**不会红**
    （时序攻击无法在功能测试里断言）→ 记 **TD-062**，由代码评审与 `subtle` 依赖保证。
- **类型**: 安全

---

### AC-S-177: 分片上传：**幂等 part** + 缺片检测 + 合并后 sha256

- **前置条件**: `storage.partSizeBytes = 1048576`（1 MiB）；一个 3 MiB 的测试文件
  （正好 3 片）；`tenant-a` 管理员。
- **测试步骤**（**主路径 + 5 条独立路径**）:
  1. `POST /api/storage/uploads`（body `{objectKey, totalParts: 3, partSize, checksum}`）；
  2. `PUT /api/storage/uploads/{id}/parts/1|2|3` 各传一片；
  3. **重传**第 2 片（内容相同）；
  4. `POST /api/storage/uploads/{id}/complete`；
  5. **路径 2**：新建一个会话，只传第 1、3 片就 `complete`；
  6. **路径 3**：新建会话，3 片全传，但 `checksum` 声明成错的值再 `complete`；
  7. **路径 4**：`abort` 一个会话后再 `complete`；
  8. **路径 5**：`complete` 后 `GET` 那个 key。
- **预期结果**:
  - 步骤 3：**200**（幂等覆盖，**不是** 409）；
  - 步骤 4：**200**，返回 `{objectKey, size, sha256}`，`size == 3145728`；
  - 步骤 5：**400 + `STORAGE_PART_MISSING`**，且 `params.missingParts`
    **逐字等于** `[2]`（⚠️ 必须**列出缺的片号**，不是只说「缺片」）；
  - 步骤 6：**409 + `STORAGE_CHECKSUM_MISMATCH`**，
    且**对象不存在**（`GET` → 404：坏文件不能被当成成功落库）；
  - 步骤 7：`abort` **204**；之后 `complete` → **404**（会话已不存在）；
  - 步骤 8：**逐字节等于**原文件（sha256 比对）；
  - **断点续传的判据**：步骤 3 重传后 `complete` 成功 → 证明「客户端不知道哪些片成功了
    就重传一遍」是安全的（这是断点续传的全部语义）。
- **类型**: 功能 / 可靠性

---

### AC-S-178: 上传大小上限 + 病毒扫描钩子

- **前置条件**: `storage.maxUploadBytes = 1048576`（1 MiB）；
  注入一个可控的 `VirusScanner` 测试替身。
- **测试步骤**:
  1. 上传 1 MiB（正好等于上限）；
  2. 上传 1 MiB + 1 字节；
  3. 用 `NoopScanner`（默认实现）上传一个文件，读响应体的 `scanVerdict`；
  4. 用「判定为感染」的替身上传一个文件；
  5. 步骤 4 之后检查落盘目录。
- **预期结果**:
  - 步骤 1：**200**（边界包含，不是「小于」）；
  - 步骤 2：**413 + `STORAGE_UPLOAD_TOO_LARGE`**，且满足**两条可断言的事实**：
    ① **响应体里带 `params.maxBytes`**（= `104857600` 之外的那个配置值，逐字等于 `1048576`）；
    ② **落盘目录里没有残留文件**（拒绝发生在写盘之前，不是「写完再删」；
    断言 `.uploads/` 与对象目录都为空）；
    ⚠️ **删掉原判据「内存峰值不随 body 增长」**：它在测试里**无法可靠断言**
    （需要测 RSS 或自定义分配器，噪声大且平台相关）。
    → 「流式」这个**实现要求**改由**代码评审**保证（`axum::extract::Multipart`
    是流式的；只要不用 `to_bytes()` 全读就是流式）；
    → 真实可断言的是「超限请求**不产生副作用**」，即上面 ②。
  - 步骤 3：`scanVerdict == "skipped"`（⚠️ **不是** `"clean"` ——
    「没扫」和「扫过且干净」是两件事，D326）；
  - 步骤 4：**422 + `STORAGE_FILE_INFECTED`**，且 `params.signature` 非空；
  - 步骤 5：**临时文件已被删除**（目录里不留残骸）；
  - **判据独立性**：步骤 3 若实现默认返回 `clean` → 红；
    步骤 4 若扫描结果不影响落库 → 红。
- **类型**: 安全

---

### AC-S-179: CSV 解析（RFC 4180）+ **Excel 显式拒绝**

- **前置条件**: 一个含以下行的 CSV（UTF-8 带 BOM）：
  ```
  name,note
  "含逗号,的值",ok
  "含""引号""的值",ok
  "含
  换行",ok
  普通,ok
  ```
- **测试步骤**（**主路径 + 5 条路径**）:
  1. 解析上面那个 CSV；
  2. **路径 2**：上传 `.xlsx`；
  3. **路径 3**：上传 `.xls`；
  4. **路径 4**：上传一个**引号未闭合**的 CSV（`"abc` 到文件末尾）；
  5. **路径 5**：上传一个单行超过 `maxRowBytes` 的 CSV；
  6. **路径 6**：上传一个空文件。
- **预期结果**:
  - 步骤 1：4 条数据行，逐字段断言：
    ① `["含逗号,的值", "ok"]`；② `["含\"引号\"的值", "ok"]`；
    ③ `["含\n换行", "ok"]`；④ `["普通", "ok"]`；
    ⚠️ BOM **不得**出现在第一行第一个字段里（`name` 不是 `\u{feff}name`）；
  - 步骤 2/3：**415 + `STORAGE_UNSUPPORTED_FORMAT`**，
    `detail` 里含 `"only csv is supported"`（⚠️ **不是静默当 CSV 解析**，
    也不是 500）；
  - 步骤 4：**400**，错误指出**行号**（不完整字段在第几行）；
  - 步骤 5：**400**，错误指出**行号**与上限；
  - 步骤 6：**200**，解析出 **0 条**数据行（空文件不是错误）。
- **类型**: 功能 / 安全

---

### AC-S-180: 异步导入：**5 万行分批 + 进度可查 + 部分成功**

- **前置条件**: 生成一个 **50001 行**的 CSV（1 行表头 + 5 万数据行），
  其中**恰好 10 行**的「数字列」写成非数字；`storage.importBatchSize = 1000`。
- **测试步骤**:
  1. `POST /api/storage/imports`（body `{objectKey, entity: "probe_rows"}`）；
  2. 立刻 `GET /api/storage/jobs/{id}`；
  3. 跑 `xtask storage worker` 直到没有待处理任务；
  4. 再 `GET /api/storage/jobs/{id}`；
  5. 查目标表的行数；
  6. 查 `platform_storage_job_error`。
- **预期结果**:
  - 步骤 1：**202 ACCEPTED**，返回 `{jobId, status: "pending"}`；
  - 步骤 2：`status == "pending"` 且 `processedRows == 0`
    （⚠️ 证明是**异步**的，不是请求里同步跑完）；
  - 步骤 4：`status == "completed"`，`processedRows == 50000`，
    `failedRows == 10`（⚠️ **两个数字都要断言**）；
  - 步骤 5：目标表里**恰好 49990 行**（合法行全入库，非法行跳过）；
  - 步骤 6：**恰好 10 条**错误回执，每条含 `rowNumber` / `column` / `reason`，
    且 `rowNumber` 是**文件里的真实行号**（1-based，含表头偏移）；
  - **进度可查的判据**（⚠️ 必须写清**怎么观察**，否则不可执行）：
    **不并发、不 sleep** 的做法 —— 把 worker 的批次上限设为 **1 批**
    （`tick(service, ctx, jobs_batch = 1)` 且 `importBatchSize = 1000`），
    然后**连续调 2 次 `tick`**：
    ① 第 1 次之后 `GET /jobs/{id}` → `0 < processedRows < 50000`（应为 **1000**）；
    ② 第 2 次之后 → `processedRows == 2000`；
    ③ 循环到 `completed`。
    ⚠️ **这样的判据才有区分力**：只在最后写一次进度的实现，
    在第 ① 步就会返回 `0` 或 `50000` → **变红**。
    ⚠️ **不要**用「起后台线程 + 轮询」的写法：那会引入 sleep 与时序不稳定。
- **类型**: 功能

---

### AC-S-181: 导入错误回执 —— **带行号与列名**，非法行跳过不入库

- **前置条件**: 一个 5 行的 CSV，其中第 3 行缺必填列、第 5 行数字非法。
- **测试步骤**:
  1. 导入；
  2. 跑 worker；
  3. 读错误回执（`GET /api/storage/jobs/{id}/errors`）；
  4. 查目标表。
- **预期结果**:
  - 步骤 3：**恰好 2 条**回执：
    ① `{rowNumber: 3, column: "email", reason: <非空>}`；
    ② `{rowNumber: 5, column: "amount", reason: <非空>}`；
    ⚠️ **列名必须是真实的列名**（`email` / `amount`），不是 `column_2` 这种下标；
    ⚠️ `reason` 是**英文 ASCII**（面向用户的中文文案由前端 i18n 渲染，契约 C6）；
  - 步骤 4：只有 3 行入库（第 1、2、4 行）；
  - ⚠️ **`rowNumber` 是文件行号**（表头是第 1 行 → 数据第一行是第 2 行），
    断言时**必须**区分「文件行号」与「数据序号」—— 这是最容易错的一处。
- **类型**: 功能

---

### AC-S-182: 导出：CSV 生成 + 异步 + 可下载

- **前置条件**: 目标表里有 **3 行**已知数据（含一个带逗号的字段、一个带换行的字段）。
- **测试步骤**:
  1. `POST /api/storage/exports`（body `{entity: "probe_rows"}`）；
  2. 跑 worker；
  3. `GET /api/storage/jobs/{id}`；
  4. 用任务返回的 `downloadUrl` 下载；
  5. 把下载到的 CSV **再用本需求的解析器**解析一遍。
- **预期结果**:
  - 步骤 1：**202**，`{jobId, status: "pending"}`；
  - 步骤 3：`status == "completed"`，且有 `downloadUrl` + `size`；
  - 步骤 4：**200**，`Content-Type: text/csv`，且带 BOM（Excel 兼容）；
  - 步骤 5：**round-trip 一致** —— 解析回来恰好 3 行，每个字段**逐字等于**原值
    （⚠️ 含逗号 / 换行的字段必须被正确**加引号与转义**，否则 round-trip 会断）；
  - **导出也要走异步**：步骤 1 之后立刻 `GET` 应是 `pending`（不是已经 `completed`）。
- **类型**: 功能

---

### AC-S-183: 任务表与对象的**租户隔离**（3 张表各验一次 RLS）

- **前置条件**: `tenant-a` 与 `tenant-b` 各有 1 个文件、1 个任务、1 条错误回执
  （由 `scripts/seed-e2e.sh` 播种）；`crates/platform-storage/manifest.json` 含 `tenantScoped`。
- **测试步骤**（**4 条**）:
  1. `tenant-a` 管理员 `GET /api/storage/objects`；
  2. `tenant-a` 管理员 `GET /api/storage/jobs`；
  3. 用 `base_app` 直连，`SET app.tenant_id = 'tenant-a'` 后 `SELECT count(*)` **四张表**；
  4. 断言 manifest 的 `tenantScoped.tables` 与 `cargo xtask tenant-policies --check`。
- **预期结果**:
  - 步骤 1/2：列表里**不含** `tenant-b` 的任何行（`tenantId` 全部逐字等于 `tenant-a`）；
  - 步骤 3：**四张表各自**只返回 `tenant-a` 的行（`object` / `upload` / `job` / `job_error`）；
  - 步骤 4：`tenantScoped.tables` **双向相等**于
    `{platform_storage_object, platform_storage_upload, platform_storage_job,
    platform_storage_job_error}`（**恰好 4 个**）；且 `--check` **退出码 0**；
    ⚠️ **判据写法**：断言要写成「与显式列出的 **4** 元素集合双向相等」，
    **不许**写成「包含这 4 个」（漏写第 4 张表时「包含 3 个」也会绿）；
    ⚠️ 步骤 3 的直连 RLS 验证也要覆盖 **4 张**（`job_error` 表同样按租户隔离）；
  - ⚠️ **为什么这 4 张表都要验**：manifest 漏声明某张表 →
    `tenant-policies --check` **会绿**（没声明就不生成也不校验）→ 那张表**完全无隔离**
    （REQ-015 的 D311 / S-04 教训）。
- **类型**: 安全

---

### AC-S-184: 权限点三处齐 / C-5 / 启动对账 / 审计埋点

- **前置条件**: 本需求新增 `crates/platform-storage/manifest.json` 与权限点种子迁移。
- **测试步骤**（**5 条**）:
  1a. `cargo xtask capabilities` **重新生成**；
  1b. `cargo test -p platform-storage --test contract_c5`；
  1c. `cargo test -p platform-server --test i18n_titles`；
  2. **只改 manifest、不加种子 INSERT**，起服务；
  3. 把 manifest 里某权限点从 `require_permission` 调用点删掉（改成非字面量），再跑 1b；
  4. 在 `crates/platform-storage/src/` 里放一个叫 `authorize` 的函数，再跑 1b；
  5. 走一遍：上传 → 删除 → 导入 → 导出，查 `platform_audit_log`。
- **预期结果**:
  - 步骤 1a：`capabilities/` 下**恰好 14 个文件**（**不增不减**，实测依据：
    `crates/xtask/src/capabilities.rs:54-81` 的 `generate` 产出 7 固定 + 7 个 sdk 包）；
    且 `permissions.json` 的 `total == 41`（**实测基线 38** + 3 个 `storage:*`），
    且 `modules` 里新增 `crates/platform-storage/manifest.json` 条目，
    且含 `storage:file:read` / `storage:file:write` / `storage:job:read` **逐字 3 条**；
  - 步骤 1b：**通过**（真 C-5 命令）；
  - 步骤 1c：**通过**，且其中**三处**必须同步更新（⚠️ **Dev 最容易漏**）：
    - **`the_title_table_has_the_pinned_number_of_rows`**
      （`crates/platform-server/tests/i18n_titles.rs:179`）硬编码了各需求的增量 →
      必须加 `const REQ_018_NEW_CODES: usize = 9;`，
      否则 `TITLES.len()` 实际 **116** 而断言仍是 107 → **红**；
    - **`all_codes()`**（同文件 `:59`）是逐 crate 手工列举 →
      必须加 `platform_storage::error::ALL_CODES`，否则新码**不在清单里**
      → `titles_cover_every_code` **检查不到它们**（**会假绿**）；
    - **`packages/i18n/test/i18n.test.ts`** 的 `codes` 数组（实测 `:102` 起，到 `:222`）
      也硬编码了全部错误码 → 必须加 9 个 `STORAGE_*`；
  - 步骤 1c 续：`packages/i18n/src/locales/{zh-CN,en-US}.json` 各加 **9** 条
    `error.STORAGE_*`（实测基线各 **107** → **116**）；
  - 步骤 2：服务**起不来**（退出码非 0），日志逐条列出缺失的权限点编码与
    来源 manifest 路径 `crates/platform-storage/manifest.json`；
  - 步骤 3：C-5 **失败**（阳性对照）；
  - 步骤 4：C-5 **失败**（`authorize` 在 C-5 命名禁区里，D216 的 8 个别名）；
  - 步骤 5：**恰好 4 个**事件码进 `platform_audit_log`
    （`storage.file.uploaded` / `storage.file.deleted` /
    `storage.import.completed` / `storage.export.completed`），
    每条含契约 C12 的固定字段集，`ChainVerifier::verify(tenant-a)` → `ok == true`。
- **类型**: 可维护性 / 安全

---

### AC-S-185.b: **明确不做**的事项（写进规格，防止「后面以为漏了」）

| 不做 | 为什么 |
|---|---|
| ❌ **S3 真连**（只能单测签名） | 无 MinIO 环境（`podman ps` 只有 postgres/redis）→ **TD-059** |
| ❌ **Excel（`.xlsx` / `.xls`）** | `.xlsx` = ZIP + XML，`zip`/`flate2`/`quick-xml` **全 0 个**在树里 → **TD-060** |
| ❌ **图片处理（缩略图 / 裁剪）** | 需图像解码库；手写 JPEG/PNG 是数万行 + 安全高风险面 → **TD-061** |
| ❌ **真病毒扫描（ClamAV）** | 只做钩子，默认 `Skipped`（**明确表示没扫**，D326） |
| ❌ **OSS / 阿里云等其它对象存储** | 只做「本地 FS + S3 协议」两份；多一份 = 多一套签名算法 |
| ❌ 报表导出 | 属 REQ-019（`demand.md` 的「不在范围内」） |

---

## 完成定义对照

| `demand.md` 的功能要求 | AC | 本轮是否真达成 |
|---|---|---|
| 对象存储抽象层：S3 / MinIO / OSS / 本地 | 175 | **本地 FS 真实现** + S3 协议（不真连）→ **TD-059** |
| 上传下载、预签名 URL、分片、断点续传 | 026 / 175 / 176 / 177 | ✅ |
| 图片处理 | — | ❌ → **TD-061** |
| 病毒扫描钩子 | 178 | ✅（钩子 + 默认 `Skipped`） |
| Excel / CSV 模板下载 | 179 / 182 | **只 CSV** → **TD-060** |
| 异步导入导出 | 180 / 182 | ✅ |
| 错误行回执 | 181 | ✅（带行号 + 列名） |
| 导入预览与校验 | 179 / 181 | ✅（解析即校验；错误带行号） |

> ⚠️ **两处明码未达成**：S3 真连（TD-059）、Excel（TD-060）、图片处理（TD-061）。
> 这不是遗漏，是**零依赖 + 无环境**下的明码代价。

---

## AC 判定（实现阶段，BA 自查）

> 判定人：BA（**同时是实现者，独立性不足**，已在 §"独立性声明"记账）。
> 独立复核由另一个 verifier 子 Agent 单独出具（`BA/feature-meta/qa-req018-impl-review.md`）。
> 判定基线：`feature/REQ-018` @ `fe9a6ed`；终版全量 **1476 passed / 0 failed / 0 ignored**（17 crate）。
> 判定口径：每条判据必须指到 `file:line` 的**具体 `assert!` + 函数名**（§9.15），不许写「测试通过」。

| AC | 结论 | 主判据（`file:line` + 函数名） | 独立性 |
|---|---|---|---|
| AC-S-026 | **不成立** | **纯函数层实现正确**：`src/object_store/presign.rs:184` `the_payload_exposes_the_expiry`（过期时间可读）、`:197` `an_expired_token_is_expired_not_invalid`、`:224` `a_ttl_above_the_maximum_is_rejected`、`:238` `a_zero_ttl_is_rejected`、`:245` `a_negative_ttl_is_rejected`。⚠️ **但步骤 1~4 全是 HTTP 行为**（`POST /presign` 返回 `{url,expiresAt}`、逐字节下载、过期 403），而**全 crate 零 HTTP 集成测试**（F-16）→ 这些判据**一条都没被验证**；下载端点 `Content-Type` 硬编码（F-06，`http.rs:460`）。 | **判据未验证** |
| AC-S-175 | **部分** | store 层扎实：`local_fs.rs:297` `put_then_get_returns_the_same_bytes`、`:313` `the_file_lands_under_the_root`、`:332` `getting_a_missing_object_reports_not_found`、`:341` `delete_is_idempotent`、`:357` `putting_the_same_key_overwrites`、`:373` `path_traversal_is_rejected_and_nothing_lands`。⚠️ **步骤 7 要求 400，实现返 404**（F-12，`service.rs:104`）；**HTTP 层零测试**（F-16）；`s3.rs:471` 有一条**恒真测试**（F-21，断言自己拼的字面量）；判据 ① 要求 `canonicalize` 前缀，实现**明确不 canonicalize**（F-12 同源）。 | **稀释** |
| AC-S-176 | **达成** | `presign.rs:252` `tampering_with_the_expiry_is_invalid`、`:271` `tampering_with_the_object_key_is_invalid`、`:284` `tampering_with_the_signature_is_invalid`、`:299` `a_token_signed_with_another_key_is_invalid`、`:340` `signature_is_checked_before_expiry`（顺序不可换，D322）；4 条路径各自独立 | 扎实 |
| AC-S-177 | **达成** | `tests/storage_real_db.rs:23` `multipart_upload_assembles_and_stores_the_object`、`:78` `missing_parts_are_reported_with_numbers`（`:116` 断言 `missingParts == [2]` 逐字）、`:123` `a_checksum_mismatch_rejects_the_object`（409 + 对象不存在）；幂等重传 `local_fs.rs:436` `reuploading_a_part_is_idempotent` | 扎实 |
| AC-S-178 | **达成** | `tests/storage_real_db.rs:169` `the_size_limit_has_no_side_effects_when_exceeded`（`:200` 413 码 + `:205` `params.maxBytes==1024` + `:212` 落盘无残留）、`:235` `the_default_scan_verdict_is_skipped`（默认 `skipped` **非** `clean`，D326）、`:253` `an_infected_file_is_rejected_and_not_stored`、`:677` `multipart_creation_rejects_an_oversized_projection` | 扎实 |
| AC-S-179 | **不成立** | 步骤 1/2 成立：`src/csv.rs:271` `the_main_path_handles_comma_quote_newline_and_bom`、Excel 拒绝 `tests/storage_real_db.rs:300` `an_xlsx_import_is_rejected_with_a_clear_code`（415 + `only csv is supported`）。⚠️ **步骤 4/5 要求「400 + 行号」，但 `create_import` 不做同步解析**（`service.rs:410-435` 只校验扩展名），解析错误发生在**异步 worker** → 任务 `failed`，**永远没有 400**；步骤 3（`.xls`）与步骤 6（空文件 200）无集成测试（F-09）。 | **虚构** |
| AC-S-180 | **未达成（部分）** | 异步与进度两条判据达成：`tests/storage_real_db.rs:340` `a_large_import_reports_progress_and_row_level_errors`：`:377` 建任务后 `status==pending`、`:378` `processed_rows==0`；`:386` 用 `tick_with_limit(..,1,1)` + `:399` 断言 `0 < processedRows < 2500` 的**中间态**（变异 M3 已确认会红）。⚠️ **但步骤 5「目标表里恰好 49990 行」未实现**：`src/worker.rs:190` 的 `Ok(()) => {}` 是**空分支**，合法行**从未写入任何目标表**；且 `probe_rows` 物理表在 `migrations/` 里**不存在**；测试规模也从 5 万降为 2500 且**未断言目标表行数**。见 F-01 / F-02。 | **稀释** |
| AC-S-181 | **未达成（部分）** | 回执行号与列名达成：`tests/storage_real_db.rs:448` `a_clean_import_has_no_error_receipts`；`src/worker.rs:429` `a_non_numeric_value_reports_the_column_name`（列名是真列名非 `column_2`）、`:474` `a_missing_column_reports_its_name`。⚠️ **但步骤 4「只有 3 行入库」未实现**（同 F-01，合法行不入库）。 | **稀释** |
| AC-S-182 | **不成立** | 生成侧成立：`src/csv.rs:385` `generate_quotes_and_escapes_special_fields`、`:400` `generate_starts_with_a_bom`、`:407` `generate_then_parse_round_trips`。⚠️ **`JobView` 没有 `size` 字段**（F-07，`model.rs:263-286`）→ 步骤 3 判据不满足；下载 `Content-Type` 不是 `text/csv`（F-06）；round-trip **未覆盖「带换行字段」**且只断言「存在」不逐字（F-08，`storage_real_db.rs:493`）。 | **虚构** |
| AC-S-183 | **部分** | 判据最强的一处：`tests/storage_real_db.rs:655` `all_four_tables_have_rls_policies`（`:663` `vec!` **双向相等** 4 元素，不是「包含」）+ `:554` `all_four_tables_are_isolated_by_tenant`；`contract_c5.rs` 同样双向相等。⚠️ 但**步骤 1/2 只测了 service 层**、没有一次 HTTP 请求（F-14）；`scripts/seed-e2e.sh` **完全不播种 storage**（F-14）。 | **部分** |
| AC-S-184 | **未达成（部分）** | 步骤 1a~1c 与 2~4 达成：`cargo xtask capabilities --check` → 退出码 0；`permissions.json` `"total": 41`；`tests/contract_c5.rs`（4 条，含阳性对照）；`i18n_titles.rs:179` 含 `REQ_018_NEW_CODES = 9`、`:59` `all_codes()` 含 `platform_storage::error::ALL_CODES`；`packages/i18n` 两库各 116。⚠️ **但步骤 5 未实现**：4 个审计事件码在 `src/events.rs:10-21` 只有常量定义，**全仓无发出点**（`grep events::` 只命中 `lib.rs:52` 的 re-export），`tests/storage_audit.rs` 不存在。见 F-03。 | **虚构** |

**判定汇总（rev 3，独立复核 FAIL 后重判）**：

| 结论 | 条数 | 是哪几条 |
|---|---|---|
| 成立 | **1** | 176 |
| 部分 | **4** | 175 / 177 / 178 / 183 |
| 不成立 | **6** | 026 / 179 / 180 / 181 / 182 / 184 |

> ⚠️ **本需求**不是**「达成」，整体判定 **FAIL**。见
> [`BA/feature-meta/qa-req018-impl-review.md`](../../feature-meta/qa-req018-impl-review.md)（22 条 FINDING：BLOCKER 3 / MAJOR 11 / MINOR 8）。

### 三次判定的修正过程（如实记账）

| 版本 | 当时结论 | 为什么错 |
|---|---|---|
| rev 1 | 「11 条全部达成，稀释 0 / 虚构 0」 | 只看「测试全绿 + 数字对得上」，**没有回到 AC 原文核对判据是否真有实现** |
| rev 2 | 「达成 8 / 稀释 2 / 虚构 1」 | 只接受了自己发现的 F-01~F-04，**没做对抗性复核**，漏掉 18 条 |
| rev 3 | **成立 1 / 部分 4 / 不成立 6** | 独立 `verifier` 出具 22 条 FINDING，我逐条亲验后确认 |

**教训**（比缺陷本身更重要）：

1. **「全量绿」只证明已写的测试通过，不证明 AC 的每条判据都写了测试**（§9.14 的「全量绿 ≠ 需求通过」再次应验）。F-02 正是**把判据删掉**才让测试变绿的。
2. **BA 自己判定自己的实现，等于没判定。** rev 1/rev 2 两次都漏，而独立复核一次就找出 22 条 —— 这不是「复核更仔细」，是**立场不同**：实现者的直觉是「我写了什么」，复核者的直觉是「规格要求什么」。
3. **数字对得上 ≠ 判据满足。** 116 / 41 / 14 这些数字全部实测一致，但它们只覆盖 AC-S-184 的步骤 1a~1c，而步骤 5 整条没做。
教训：**「全量绿」只证明已写的测试通过，不证明 AC 的每条判据都写了测试**（§9.14 的「全量绿 ≠ 需求通过」再次应验）。

### 独立性声明（D122 / §9.15）

本轮 **BA 兼任实现者**，这是本项目一贯要求避免的模式。如实记账：

1. **AC 判定由实现者本人出具** → 存在确认偏误风险。缓解措施：另派独立 `verifier` 子 Agent 做对抗性复核，**其结论单独成文**，不并入本表。
2. **挑刺也是 BA 自查**（`qa-req018-spec-review.md` 的 8 条）→ 同样受确认偏误影响。缓解：8 条挑刺全部**真修**（有 diff 可查），且其中 4 条属于「规格写了判据但不可执行」这类**能靠机械核对发现**的问题。
3. **可以降低风险的部分**：终版全量的数字（1476 / 141 / 116 / 41）全部由命令实测得出，非人工计数；4 个变异**真跑过并确认变红**，这属于机制性证据，不受判定人主观影响。
