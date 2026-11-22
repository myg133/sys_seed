# 01 对象存储与预签名（AC-S-026 / 175 / 176）

> 测试落点：`crates/platform-storage/src/object_store/presign.rs` 内联单测（026/176）+
> `tests/storage_local_fs.rs`（175）。

### TC-018-001 预签名签发 → 199? 不，200 + expiresAt 在 1~2 秒窗口内
- **AC**: AC-S-026 步骤 1 ｜ **类型**: 功能 ｜ **优先级**: P0 ｜ **自动化**: 单测（纯函数）｜ **方法**: 边界值
- **前置**: 一个已知 `objectKey = "docs/a.txt"`；`presignTtlSeconds = 900`（默认）
- **步骤**: 1) 调 `presign_issue(key, ttl=2, now=T0)` 2) 读返回的 `expires_at`
- **预期**: ① 返回值是 `Ok`；② `expires_at == T0 + 2s`（**逐秒相等**，不是区间）；
  ③ token 结构是 `base64url(payload) + "." + base64url(sig)`（三段：含两个点）

### TC-018-002 有效期内下载 → 200 且内容逐字节相同
- **AC**: AC-S-026 步骤 2 ｜ P0 ｜ 集成（真临时目录）｜ 等价类
- **前置**: 已 `put("docs/a.txt", <1 MiB 随机字节>)`，记下 sha256
- **步骤**: 1) 签 ttl=60 2) 用 token 读 3) 算 sha256
- **预期**: ① 读到 `Ok(bytes)`；② `sha256(bytes)` **逐字节等于**原文件摘要
  （⚠️ 不是比长度）；③ 断言 `bytes.len() == 1048576`

### TC-018-003 过期后下载 → 403 STORAGE_PRESIGN_EXPIRED
- **AC**: AC-S-026 步骤 3 ｜ P0 ｜ 单测（**注入时钟**，不 sleep）｜ 边界值
- **前置**: 签 ttl=2，`now = T0`
- **步骤**: 用 `now = T0 + 3s` 校验
- **预期**: `Err`，码 **逐字** `STORAGE_PRESIGN_EXPIRED`；
  ⚠️ **不得**是 `STORAGE_PRESIGN_INVALID`（签名是对的，只是过期）

### TC-018-004 token 里能读过期时间（可排障）
- **AC**: AC-S-026 步骤 4 ｜ P1 ｜ 单测 ｜ 等价类
- **步骤**: 签 ttl=900，base64url 解码 token 的第一段（payload）
- **预期**: payload 里有 `expires_at` 字段，值 == 签发时刻 + 900；
  ⚠️ 断言方式：`serde_json::from_str` 能解出结构体且字段存在（不是「字符串里含数字」）

### TC-018-005 TTL 超上限 → 400 STORAGE_TTL_OUT_OF_RANGE（拒绝而非截断）
- **AC**: AC-S-026 步骤 5 ｜ P0 ｜ 单测 ｜ 边界值
- **步骤**: 1) `ttl = 86400 * 365` 2) `ttl = 86401`（上限+1）3) `ttl = 86400`（正好上限）
- **预期**: ① 两次都 `Err` + `STORAGE_TTL_OUT_OF_RANGE`；
  ② ⚠️ **不签发**（返回 `Err` 就是没签，不许「截断到 86400 再签」）；
  ③ 第 3 个**成功**（边界包含，阳性对照）

### TC-018-006 TTL = 0 → STORAGE_TTL_OUT_OF_RANGE（0 是非法，不是立即过期）
- **AC**: AC-S-026 步骤 5 ｜ P0 ｜ 单测 ｜ 边界值
- **预期**: `Err` + `STORAGE_TTL_OUT_OF_RANGE`；
  ⚠️ **不得**签一个「立即过期」的 token（那会让调用方以为成功）

### TC-018-007 篡改过期时间 → 403 STORAGE_PRESIGN_INVALID
- **AC**: AC-S-176 步骤 1 ｜ P0 ｜ 单测 ｜ 错误推测
- **步骤**: 1) 签 ttl=2 2) 解 payload 3) 把 `expires_at` 改成 +1 天 4) 重编码 5) 校验
- **预期**: `Err` + **逐字** `STORAGE_PRESIGN_INVALID`
  （⚠️ **不是** EXPIRED、**不是** 200 —— 改时间必须被识别为签名不符）

### TC-018-008 篡改 object_key → 403 STORAGE_PRESIGN_INVALID
- **AC**: AC-S-176 步骤 2 ｜ P0 ｜ 单测 ｜ 错误推测
- **步骤**: 1) 签 `docs/a.txt` 2) 把 payload 里的 key 改成 `docs/secret.txt` 3) 校验
- **预期**: `Err` + `STORAGE_PRESIGN_INVALID`（key 在签名覆盖范围内）；
  ⚠️ 若实现把 key 只放在 URL path 里而不签进去 → 本条红。**这是关键判据**

### TC-018-009 篡改签名一个字符 → 403 STORAGE_PRESIGN_INVALID
- **AC**: AC-S-176 步骤 3 ｜ P0 ｜ 单测 ｜ 错误推测
- **步骤**: 把 token 第二段（sig）的**第一个字符**改成另一个 base64url 字符
- **预期**: `Err` + `STORAGE_PRESIGN_INVALID`

### TC-018-010 判据独立性：4 条篡改用例互不覆盖
- **AC**: AC-S-176「判据独立性」 ｜ P0 ｜ 单测 ｜ 因果
- **步骤**: 分别做 4 个变异（不改实现，只验证用例）：
  ① 实现不签 `expires_at` → TC-007 必须红；② 不签 `object_key` → TC-008 红；
  ③ 签名改一个字符 → TC-009 红；④ 过期 token → TC-003 红
- **预期**: 4 条各自独立变红（用 `#[ignore]` 的变异开关或直接改代码后手工确认）

### TC-018-011 主路径：PUT 1 MiB → GET 逐字节相同
- **AC**: AC-S-175 步骤 1-2 ｜ P0 ｜ 集成（真临时目录）｜ 等价类
- **前置**: `BASE_STORAGE_ROOT` = 临时目录
- **预期**: ① `PUT` 返回 200；② `GET` 的 sha256 **逐字节等于**上传时的摘要

### TC-018-012 落盘文件真的在 root 下
- **AC**: AC-S-175 步骤 3 ｜ P0 ｜ 集成 ｜ 等价类
- **步骤**: `PUT` 后遍历 `BASE_STORAGE_ROOT` 找该文件
- **预期**: 找到**恰好一个**文件；其 `canonicalize()` 以 `canonicalize(root)` 为前缀

### TC-018-013 GET 不存在的 key → 404 STORAGE_OBJECT_NOT_FOUND
- **AC**: AC-S-175 步骤 4 ｜ P0 ｜ 集成 ｜ 等价类
- **预期**: 404 + **逐字** `STORAGE_OBJECT_NOT_FOUND`

### TC-018-014 DELETE → 204，再 GET → 404
- **AC**: AC-S-175 步骤 5 ｜ P0 ｜ 集成 ｜ 状态迁移
- **预期**: ① DELETE **204**（不是 200）；② 之后 GET **404**

### TC-018-015 同 key 再 PUT → 覆盖
- **AC**: AC-S-175 步骤 6 ｜ P0 ｜ 集成 ｜ 状态迁移
- **步骤**: PUT 内容 A → PUT 内容 B → GET
- **预期**: 两次都 200；GET 得 **B**（逐字节），且 A 的长度与 B 不同以便区分

### TC-018-016 路径穿越 `docs/../../etc/passwd` → 400 且所有落盘在 root 内
- **AC**: AC-S-175 步骤 7 ｜ **P0（安全）** ｜ 集成 ｜ 错误推测
- **前置**: 在 root 的**父目录**放一个哨兵文件 `sentinel.txt`（内容已知）
- **步骤**: 1) `PUT` key = `docs/../../etc/passwd` 2) 遍历 root
- **预期**: ① 返回 **400**；② **所有**落盘路径以 `canonicalize(root)` 为前缀；
  ③ 哨兵文件**内容未变**（阳性对照）

### TC-018-017 S3 SigV4 签名按 AWS 官方向量验证
- **AC**: AC-S-175「S3Store 的存在性」 ｜ P1 ｜ 单测 ｜ 等价类
- **前置**: AWS SigV4 官方测试向量（`get-vanilla` 用例：已知 key/region/时间/期望签名）
- **步骤**: 用我们的实现算该向量的签名
- **预期**: 签名**逐字节等于**官方向量里的 `expectedAuthorization`
- ⚠️ **本条只证明签名算法对，不证明 S3 可用**（TD-059）

---

# 02 分片上传（AC-S-177）

### TC-018-018 分片上传主路径：3 片 → complete → sha256 一致
- **AC**: AC-S-177 步骤 1-4 ｜ P0 ｜ 集成 ｜ 等价类
- **前置**: `partSizeBytes = 1048576`；3 MiB 文件；预声明 sha256
- **预期**: ① 建会话返回 `uploadId`；② 3 次 PUT 各 200；
  ③ complete 返回 `{size: 3145728, sha256}`；④ sha256 逐字节等于原文件

### TC-018-019 重传同一 part → 200（幂等）
- **AC**: AC-S-177 步骤 3 ｜ P0 ｜ 集成 ｜ 幂等
- **预期**: 重传**同一片**返回 **200**（⚠️ 不是 409）；
  complete 后内容仍正确（证明覆盖是安全的）

### TC-018-020 缺片 complete → 400 STORAGE_PART_MISSING 且列出缺的片号
- **AC**: AC-S-177 步骤 5 ｜ P0 ｜ 集成 ｜ 边界值
- **步骤**: 只传第 1、3 片 → complete
- **预期**: ① **400**；② 码 **逐字** `STORAGE_PART_MISSING`；
  ③ `params.missingParts` **逐字等于** `[2]`（⚠️ 必须列片号，不能只说「缺片」）

### TC-018-021 checksum 不符 → 409 STORAGE_CHECKSUM_MISMATCH 且对象不存在
- **AC**: AC-S-177 步骤 6 ｜ **P0（安全）** ｜ 集成 ｜ 错误推测
- **步骤**: 3 片全传但声明错 checksum → complete → GET
- **预期**: ① **409** + `STORAGE_CHECKSUM_MISMATCH`；
  ② ⚠️ **对象不存在**：`GET` → **404**（坏文件绝不能落库）

### TC-018-022 abort 后 complete → 404
- **AC**: AC-S-177 步骤 7 ｜ P1 ｜ 集成 ｜ 状态迁移
- **预期**: ① abort **204**；② 之后 complete → **404**（会话已不存在）

### TC-018-023 complete 后 GET 内容正确
- **AC**: AC-S-177 步骤 8 ｜ P0 ｜ 集成 ｜ 等价类
- **预期**: sha256 逐字节等于原文件

### TC-018-024 并发上传同一 part → 后写的赢，永不读到半个文件
- **AC**: AC-S-177 + design §2.2（S-05）｜ P1 ｜ 集成 ｜ 因果
- **步骤**: 两个 task 同时 PUT 同一 part（不同内容），用 `Barrier` 同步开始
- **预期**: ① 两次都 200；② 最终 `part-<n>` 的内容**完整等于**其中一个（不是混合）；
  ③ complete 后的 sha256 等于「赢的那份」的摘要

---

# 03 上传限制与病毒扫描（AC-S-178）

### TC-018-025 正好等于 maxUploadBytes → 200（边界包含）
- **AC**: AC-S-178 步骤 1 ｜ P0 ｜ 集成 ｜ 边界值
- **前置**: `maxUploadBytes = 1048576`
- **预期**: 上传**恰好 1 MiB** → **200**

### TC-018-026 超 1 字节 → 413 且落盘无残留
- **AC**: AC-S-178 步骤 2 ｜ **P0（安全）** ｜ 集成 ｜ 边界值
- **步骤**: 上传 1 MiB + 1 字节
- **预期**: ① **413** + `STORAGE_UPLOAD_TOO_LARGE`；
  ② `params.maxBytes` **逐字等于** `"1048576"`；
  ③ ⚠️ **落盘目录为空**（拒绝在写盘之前，不是「写完再删」）——
  遍历 root 断言文件数为 0

### TC-018-027 默认扫描器 → scanVerdict 是 skipped（不是 clean）
- **AC**: AC-S-178 步骤 3 ｜ **P0（安全）** ｜ 集成 ｜ 等价类
- **预期**: 响应体 `scanVerdict == "skipped"`；
  ⚠️ **不得**是 `"clean"` —— 「没扫」与「扫过且干净」是两件事（D326）

### TC-018-028 判定感染 → 422 且临时文件被删
- **AC**: AC-S-178 步骤 4-5 ｜ **P0（安全）** ｜ 集成 ｜ 错误推测
- **前置**: 注入一个恒返回 `Infected { signature: "EICAR-Test" }` 的替身
- **预期**: ① **422** + `STORAGE_FILE_INFECTED`；② `params.signature` 非空；
  ③ ⚠️ 落盘目录**无残留**（临时文件已删）

### TC-018-029 判据独立性：扫描结果必须影响落库
- **AC**: AC-S-178「判据独立性」 ｜ P0 ｜ 集成 ｜ 因果
- **步骤**: 变异：把 `infected` 分支改成「照样落库」
- **预期**: TC-028 变红（证明扫描不是装饰）

---

# 04 CSV 解析（AC-S-179）

### TC-018-030 CSV 主路径：逗号 / 引号 / 换行 / BOM 四种边界
- **AC**: AC-S-179 步骤 1 ｜ **P0** ｜ 单测（纯函数）｜ 边界值
- **前置**: 输入（UTF-8 **带 BOM**）：
  ```
  name,note
  "含逗号,的值",ok
  "含""引号""的值",ok
  "含
  换行",ok
  普通,ok
  ```
- **预期**: 恰好 **4** 条数据行，逐字段断言：
  ① `["含逗号,的值", "ok"]`；② `["含\"引号\"的值", "ok"]`；
  ③ `["含\n换行", "ok"]`；④ `["普通", "ok"]`；
  ⚠️ 第一行第一个字段是 `name`，**不是** `\u{feff}name`（BOM 必须被剥掉）

### TC-018-031 上传 .xlsx → 415 STORAGE_UNSUPPORTED_FORMAT
- **AC**: AC-S-179 步骤 2 ｜ P0 ｜ 集成 ｜ 错误推测
- **预期**: ① **415**；② 码 `STORAGE_UNSUPPORTED_FORMAT`；
  ③ `detail` 含 `"only csv is supported"`；
  ⚠️ **不是** 500、**不是**把 xlsx 当 CSV 解析（那会得到乱码行）

### TC-018-032 上传 .xls → 415（同上）
- **AC**: AC-S-179 步骤 3 ｜ P0 ｜ 集成 ｜ 等价类
- **预期**: 同 TC-031

### TC-018-033 引号未闭合 → 400 且指出起始行号
- **AC**: AC-S-179 步骤 4 ｜ **P0（安全）** ｜ 单测 ｜ 错误推测
- **前置**: 内容 `name,note\n"abc`（第 2 行引号未闭合到文件末尾）
- **预期**: `Err`，错误里含**行号 2**；
  ⚠️ **不得**把整个剩余文件当一个字段（那会让 `maxRowBytes` 失效 → 内存爆）

### TC-018-034 单行超 maxRowBytes → 400 且指出行号与上限
- **AC**: AC-S-179 步骤 5 ｜ P0 ｜ 单测 ｜ 边界值
- **前置**: `maxRowBytes = 64`；第 3 行有 100 字节
- **预期**: `Err`，含**行号 3** 与上限 `64`

### TC-018-035 空文件 → 0 条数据行（不是错误）
- **AC**: AC-S-179 步骤 6 ｜ P1 ｜ 单测 ｜ 边界值
- **预期**: `Ok(vec![])`；⚠️ 不是 `Err`

### TC-018-036 CSV 生成 → 含逗号 / 换行的字段被加引号并转义
- **AC**: AC-S-182 步骤 5（round-trip 的前提）｜ P0 ｜ 单测 ｜ 等价类
- **步骤**: 生成 `[["a,b", "c\"d"], ["e\nf", "g"]]`
- **预期**: 输出含 `"a,b"` 与 `"c""d"` 与 `"e\nf"`（引号正确加倍）

---

# 05 异步导入（AC-S-180 / 181）

### TC-018-037 导入 202 → 立刻查是 pending 且 processedRows == 0
- **AC**: AC-S-180 步骤 1-2 ｜ P0 ｜ 集成 ｜ 等价类
- **预期**: ① **202**；② 返回 `{jobId, status: "pending"}`；
  ③ 立刻 GET：`processedRows == 0`（⚠️ 证明是异步，不是请求里跑完）

### TC-018-038 5 万行导入完成 → processedRows 50000 / failedRows 10
- **AC**: AC-S-180 步骤 3-4 ｜ **P0** ｜ 集成（真库）｜ 等价类
- **前置**: 50001 行 CSV（表头 + 5 万数据），其中恰好 10 行的数字列非法
- **步骤**: 反复 `tick` 到没有待处理任务
- **预期**: ① `status == "completed"`；② ⚠️ **`processedRows == 50000`**；
  ③ ⚠️ **`failedRows == 10`**（**两个数字都要断言**）

### TC-018-039 合法行全入库：目标表恰好 49990 行
- **AC**: AC-S-180 步骤 5 ｜ P0 ｜ 集成 ｜ 等价类
- **预期**: `SELECT count(*)` == **49990**

### TC-018-040 进度可查：连调 2 次 tick 观察中间态
- **AC**: AC-S-180「进度可查的判据」 ｜ **P0（区分力核心）** ｜ 集成 ｜ 因果
- **前置**: `importBatchSize = 1000`；`tick` 的 jobs_batch = 1
- **步骤**: 1) 第 1 次 tick 后 GET 2) 第 2 次 tick 后 GET 3) 循环到完成
- **预期**: ① 第 1 次后 `processedRows == 1000`（**0 < 1000 < 50000**）；
  ② 第 2 次后 `== 2000`；③ 最终 50000
  ⚠️ **只在最后写一次进度的实现在 ① 就返回 0 或 50000 → 必红**

### TC-018-041 错误回执恰好 10 条，带行号与列名
- **AC**: AC-S-180 步骤 6 / AC-S-181 ｜ **P0** ｜ 集成 ｜ 等价类
- **预期**: ① 恰好 **10** 条；② 每条含 `rowNumber` / `column` / `reason`；
  ③ `column` 是**真实列名**（如 `amount`），不是 `column_2`；
  ④ `reason` 是 **ASCII**（`is_ascii()` 为 true）

### TC-018-042 行号是「文件行号」（含表头偏移）
- **AC**: AC-S-181 ｜ **P0** ｜ 集成 ｜ 边界值
- **前置**: 5 行文件（1 表头 + 4 数据），第 3 行缺 `email`、第 5 行 `amount` 非法
- **预期**: 恰好 2 条回执：`{rowNumber: 3, column: "email"}` 与
  `{rowNumber: 5, column: "amount"}`；
  ⚠️ 文件行号 ≠ 数据序号（数据第 2 行 = 文件第 3 行）

### TC-018-043 非法行跳过但前面已入库（部分成功）
- **AC**: AC-S-181 步骤 4 ｜ P0 ｜ 集成 ｜ 状态迁移
- **预期**: 4 条数据里 2 条合法 → 表里**恰好 2 行**（第 1、4 行对应的数据）

### TC-018-044 空 CSV 导入 → completed 且 0 行 0 错误
- **AC**: AC-S-179 步骤 6 的延伸 ｜ P1 ｜ 集成 ｜ 边界值
- **预期**: `processedRows == 0`、`failedRows == 0`、`status == "completed"`

---

# 06 导出（AC-S-182）

### TC-018-045 导出 202 → 立刻是 pending
- **AC**: AC-S-182 步骤 1 ｜ P0 ｜ 集成 ｜ 等价类
- **预期**: **202** + `{jobId, status: "pending"}`；立刻 GET 仍是 `pending`
  （⚠️ 证明导出也是异步）

### TC-018-046 导出完成 → downloadUrl + size + text/csv + BOM
- **AC**: AC-S-182 步骤 3-4 ｜ P0 ｜ 集成 ｜ 等价类
- **预期**: ① `status == "completed"`；② `downloadUrl` 非空；③ `size > 0`；
  ④ 下载响应 `Content-Type: text/csv`；⑤ ⚠️ 前 3 字节是 `EF BB BF`（BOM）

### TC-018-047 导出 round-trip：下载再用本需求解析器解析，逐字段一致
- **AC**: AC-S-182 步骤 5 ｜ **P0（区分力核心）** ｜ 集成 ｜ 因果
- **前置**: 表里 3 行，含一个**带逗号**的字段、一个**带换行**的字段
- **预期**: 解析回来恰好 3 行，每个字段**逐字等于**原值；
  ⚠️ 若生成时没加引号/转义 → 解析回来会多出列或错行 → 本条红

---

# 07 租户隔离与权限（AC-S-183 / 184）

### TC-018-048 对象列表不含跨租户行
- **AC**: AC-S-183 步骤 1 ｜ P0 ｜ 集成（应用角色）｜ 等价类
- **预期**: `tenant-a` 的列表里 `tenantId` **全部逐字等于** `tenant-a`

### TC-018-049 任务列表不含跨租户行
- **AC**: AC-S-183 步骤 2 ｜ P0 ｜ 集成 ｜ 等价类
- **预期**: 同上

### TC-018-050 四张表各自验证 RLS
- **AC**: AC-S-183 步骤 3 ｜ **P0（安全）** ｜ 集成（应用角色直连）｜ 等价类
- **步骤**: `SET app.tenant_id = 'tenant-a'` 后对**四张表**各 `SELECT count(*)`
- **预期**: ① `platform_storage_object` 只返回 1 行；
  ② `platform_storage_upload` 只返回 1 行；③ `platform_storage_job` 只返回 1 行；
  ④ `platform_storage_job_error` 只返回 1 行
  ⚠️ **四张都要**（`job_error` 同样按租户隔离）

### TC-018-051 manifest 的 tenantScoped 恰好 4 张表
- **AC**: AC-S-183 步骤 4 ｜ **P0** ｜ 单测（读 manifest）｜ 等价类
- **预期**: `tenantScoped.tables` **双向相等**于
  `{platform_storage_object, platform_storage_upload, platform_storage_job,
  platform_storage_job_error}`（**恰好 4 个**）；
  ⚠️ 断言写法是「与显式 4 元素集合相等」，**不是**「包含这 4 个」

### TC-018-052 tenant-policies --check 退出码 0
- **AC**: AC-S-183 步骤 4 ｜ P0 ｜ 命令 ｜ 等价类
- **预期**: `DATABASE_URL=$DATABASE_URL_TEST cargo xtask tenant-policies --check` 退出码 **0**

### TC-018-053 capabilities 恰好 14 文件 + total == 41
- **AC**: AC-S-184 步骤 1a ｜ P0 ｜ 命令 ｜ 等价类
- **预期**: ① `find capabilities -type f | wc -l` == **14**；
  ② `permissions.json` 的 `total` == **41**；
  ③ `modules` 里有 `crates/platform-storage/manifest.json` 条目；
  ④ 含 3 个 `storage:*` 逐字

### TC-018-054 contract_c5 通过（3 个权限点各有字面量调用点）
- **AC**: AC-S-184 步骤 1b ｜ P0 ｜ 契约 ｜ 等价类
- **预期**: `cargo test -p platform-storage --test contract_c5` 全绿；
  且 `scan.codes` 恰好是那 3 个（不多不少）

### TC-018-055 i18n_titles 通过（含 all_codes 与钉死行数）
- **AC**: AC-S-184 步骤 1c ｜ **P0** ｜ 契约 ｜ 等价类
- **预期**: ① `i18n_titles` 全绿；② **源码 grep** 断言
  `platform_storage::error::ALL_CODES` 在 `all_codes()` 里（⚠️ 不能只看测试绿 ——
  漏加时它恰好是绿的）；③ `REQ_018_NEW_CODES = 9` 存在

### TC-018-056 只改 manifest 不加种子 → 服务起不来
- **AC**: AC-S-184 步骤 2 ｜ P1 ｜ 命令 ｜ 错误推测
- **步骤**: 删掉迁移里的权限点 INSERT，起服务
- **预期**: 退出码非 0；日志**逐条**列出缺失的权限点编码与
  `crates/platform-storage/manifest.json`

### TC-018-057 C-5 阳性对照：删掉一个 require_permission 调用点
- **AC**: AC-S-184 步骤 3 ｜ P0 ｜ 契约 ｜ 因果
- **预期**: contract_c5 **失败**（已登记但无调用点）

### TC-018-058 C-5 阳性对照：放一个叫 authorize 的函数
- **AC**: AC-S-184 步骤 4 ｜ P0 ｜ 契约 ｜ 因果
- **预期**: contract_c5 **失败**（`authorize` 在 D216 的命名禁区里）

### TC-018-059 四个审计事件码进 audit_log 且链自洽
- **AC**: AC-S-184 步骤 5 ｜ P0 ｜ 集成 ｜ 等价类
- **步骤**: 上传 → 删除 → 导入 → 导出
- **预期**: ① 恰好 4 个事件码（`storage.file.uploaded` / `storage.file.deleted` /
  `storage.import.completed` / `storage.export.completed`）；
  ② 每条含契约 C12 的固定字段集；③ `ChainVerifier::verify(tenant-a)` → `ok == true`

---

## 〔AC 修正〕清单

**未修改** `acceptance.md`（除了 BA 自查已修的 S-01~S-08）。本集在用例里给出**加强**的判据：

| # | AC 位置 | 加强点 | 用例 |
|---|---|---|---|
| 1 | AC-S-178 步骤 2 | AC 已删「内存峰值」判据；本集进一步明确「遍历 root 断言文件数 0」 | TC-026 |
| 2 | AC-S-180 进度判据 | 本集定死「jobs_batch=1 + importBatchSize=1000 → 每次 +1000」 | TC-040 |
| 3 | AC-S-183 步骤 3 | 本集要求**四张表各自**断言，不是「抽查三张」 | TC-050 |

## 与设计的一致性核对

用例引用的表名 / 字段 / 端点全部取自 `design-summary.md`，未自造：
- 4 张表：`platform_storage_object` / `_upload` / `_job` / `_job_error`（§2）
- 12 个路由（§3）+ 1 个免鉴权下载端点
- camelCase 字段：`objectKey` / `sizeBytes` / `sha256` / `contentType` / `scanVerdict` /
  `uploadId` / `totalParts` / `partSize` / `jobId` / `kind` / `totalRows` /
  `processedRows` / `failedRows` / `rowNumber` / `column` / `reason`（§3.1）
- 9 个错误码与 HTTP 状态（§5）
- 6 个配置项与默认值（§6）
