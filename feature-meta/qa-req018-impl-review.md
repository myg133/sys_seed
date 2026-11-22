# REQ-018 实现阶段独立复核

> 复核者：独立 verifier 子 Agent（非实现者）
> 复核对象：`feature/REQ-018` @ `fe9a6ed`（对比基线 `develop` @ `c27528a`）
> 状态：**已完成** —— 结论 **FAIL**（3 BLOCKER / 11 MAJOR / 8 MINOR）

## FINDING 清单

| # | 严重级 | 位置 | 问题 | 判据 |
|---|---|---|---|---|
| F-01 | **BLOCKER** | `crates/platform-storage/src/worker.rs:189-199` | 导入的**合法行从未写入目标表**：`Ok(()) => {}` 空分支，全仓无任何「entity → 目标表」的 INSERT 路径（`grep "INSERT INTO"` 只命中 object/upload/job/job_error 四张元数据表；`grep probe_rows` 只命中 `model.rs:531` 的单测字面量）。 | AC-S-180 步骤 5「目标表里**恰好 49990 行**（合法行全入库）」、AC-S-181 步骤 4「只有 3 行入库」——两条核心判据的实现不存在。这是 D325「每批一事务 insert_batch」的直接缺失。 |
| F-02 | **BLOCKER** | `crates/platform-storage/tests/storage_real_db.rs:339-444` | AC-S-180 的 5 万行判据被替换成 **2500 行**，且**完全没有断言目标表行数**（无 `SELECT count(*) FROM probe_rows`）。测试只断言 `processed_rows == 2500` / `failed_rows == 5` / 回执行号，即只验了「数了行」，没验「入了库」。 | F-01 因此**逃过测试**：把 AC-S-180 步骤 5（49990）与 AC-S-181 步骤 4（3 行入库）两条判据删掉后测试仍全绿。注释 `:336-338` 自认「规模差异在报告里如实说明」，但报告未说明，且判据不是「规模」问题而是「整条判据消失」。 |
| F-03 | **BLOCKER** | `crates/platform-storage/src/events.rs:10-21`（无发出点） | 4 个审计事件码**从未被发出**：全仓 `grep "storage.file.uploaded"` 等只命中常量定义；`grep -rn "audit_log\|AuditWriter\|append_audit" crates/platform-storage/` **零命中**；`src/` 里唯一用到 `events::` 的只有 `lib.rs:52` 的 re-export。design §9 指定的 `tests/storage_audit.rs` **不存在**（`tests/` 只有 `contract_c5.rs` / `storage_real_db.rs` / `common/`）。 | AC-S-184 步骤 5「**恰好 4 个**事件码进 `platform_audit_log` … `ChainVerifier::verify(tenant-a)` → `ok == true`」整条未实现、未测。对比 `platform-server/tests/audit_chain_real_db.rs` / `app_real_stack.rs:749` 有真库落审计的基线形态 —— 本需求缺该层。 |
| F-04 | **MAJOR** | `crates/platform-storage/src/config.rs:136`（`settings.part_size_bytes`） | `storage.partSizeBytes` 是**死配置**：`StorageSettings.part_size_bytes` 只被 `config.rs:136` 填充、被单测断言默认值，业务代码**零读取**（`grep settings.part_size_bytes` 在 `src/` 内只命中 config.rs 自身）。`create_upload` 用的是**客户端传入**的 `body.part_size`（`service.rs:271`）。 | D331 要求「6 个配置项**每个必须被真实读取**」；AC-S-177 前置条件写「`storage.partSizeBytes = 1048576`」暗示服务端应据它约束分片大小。改这个配置项对行为**无影响**。 |
| F-05 | **MAJOR** | `crates/platform-storage/src/http.rs:368` / `:504` / `:335` | 上传路径**不是流式**：`axum::body::to_bytes(request.into_body(), 256MiB)` 把整个 body 读进内存，再交给 `put_object` 做大小校验（`service.rs:113-120`）。multipart 路径用 `field.bytes()` 同样全量缓冲。 | AC-S-178 判据说明「**流式**这个实现要求改由**代码评审**保证（只要不用 `to_bytes()` 全读就是流式）」—— 实现恰好用了 `to_bytes()`。后果：一个 200 MiB 请求在拒绝前已吃 200 MiB 内存（`maxUploadBytes` 默认 100 MiB 拦不住缓冲）。注释 `service.rs:96`「① 大小校验在写盘之前」在**写盘**意义上成立，但在**内存**意义上不成立。 补充已核实事实：body 超过 `to_bytes` 的 **256 MiB** 硬上限时走 `Err` → `ServiceError::Internal` → **500**，而不是 413（`http.rs:370-378`）。 |
| F-06 | **MAJOR** | `crates/platform-storage/src/http.rs:460-463` | 预签名下载端点把 `Content-Type` **硬编码**为 `application/octet-stream`，不读对象的 `content_type`。 | AC-S-182 步骤 4「**200**，`Content-Type: text/csv`，且带 BOM」——导出结果经 `downloadUrl` 下载时返回的是 `application/octet-stream`，不是 `text/csv`。同一端点也是 AC-S-026 步骤 2（逐字节下载）的落点。无任何测试断言该响应头。 |
| F-07 | **MAJOR** | `crates/platform-storage/src/model.rs:263-286`（`JobView`） | `JobView` **没有 `size` 字段**（只有 `downloadUrl`），`view_of_job` 也不填。 | AC-S-182 步骤 3「`status == "completed"`，且有 `downloadUrl` **+ `size`**」——`size` 判据无对应字段，无法满足。 |
| F-08 | **MAJOR** | `crates/platform-storage/tests/storage_real_db.rs:491-550` | AC-S-182 的 round-trip 只覆盖**含逗号**的 key（`with,comma.txt`），**没有含换行**的字段；规格前置条件要求「含一个带逗号的字段、**一个带换行的字段**」，且步骤 5 要求「每个字段**逐字等于原值**」。测试用 `keys.iter().any(...)` 只断言「存在」，未逐字段比对。 | AC-S-182 步骤 5 判据被稀释成「至少有一个含逗号的值能读回」。含换行字段的转义**完全未验证**（`csv.rs` 单测 `generate_then_parse_round_trips` 有覆盖，但那不是导出链路的落点）。 |
| F-09 | **MAJOR** | `crates/platform-storage/tests/storage_real_db.rs:298-332` | AC-S-179 **步骤 3（`.xls`）无集成测试**：`storage_real_db.rs` 只有 `.xlsx` 一条；`.xls` 仅出现在 `service.rs:519` 的**纯函数单测**里，没有走 `create_import` 端到端。AC-S-179 步骤 6（空文件 → 200 + 0 行）也无集成用例（只有 `csv.rs` 单测）。 | AC-S-179 明确列了「主路径 + 5 条路径」，步骤 2 与 3 是**两条独立路径**（`.xlsx` / `.xls`）。步骤 6 的「200 而不是错误」在 HTTP/服务层无断言。 |
| F-10 | **MAJOR** | `crates/platform-storage/src/worker.rs:186-187` + `src/csv.rs:201`、`csv.rs:141-157` | **行号会漂移**：(a) `csv::parse` 在 `csv.rs:201` 用 `records.retain` 删空行后数据行被重新编号，`index+2` 不再等于文件行号；(b) `csv.rs:141-157` 的引号分支对字段内的换行**不递增 `line`**，所以含换行字段之后的行号也偏小（AC-S-179 步骤 1 的样例恰好含一个多行字段）。 | AC-S-181「`rowNumber` 是**文件行号**（1-based，含表头偏移）」。现有测试的 CSV 既无空行也无多行字段，两个偏差都测不到；一旦输入含空行/多行字段，`platform_storage_job_error.row_number` 就不再是文件行号。 |
| F-11 | **MAJOR** | `crates/platform-storage/src/worker.rs:359-367` | `validate_row` 的「列名必须存在」检查与 `to_map` 同源（`csv.rs:100-106` 用 `header.zip(row)`），而 `csv::parse` 已保证每行列数 == 表头列数 → **这个分支不可达**（`map.contains_key` 恒真）。真要「缺必填列」只能靠列数检查进 `ColumnCountMismatch`，那是**整文件报错**。 | AC-S-181 步骤 3 要求「`{rowNumber: 3, column: "email", reason: <非空>}`」——「第 3 行缺必填列」在**列数一致**的 CSV 里不可能被判出；若列数不一致则走 `CsvError::ColumnCountMismatch` → `worker.rs:153-154` 直接 `Err` → 整个任务 failed，**不产生任何错误回执**。AC-S-181 的场景（2 条回执 + 3 行入库）无测试、无实现路径。 |
| F-12 | **MINOR** | `crates/platform-storage/src/service.rs:104-108` | 路径穿越 key 返回 `StorageError::ObjectNotFound` → **404**。 | AC-S-175 步骤 7 明确要求「**400**」。404 虽不泄露信息，但与规格的判据冲突；`local_fs.rs:373-385` 的测试只断言 `is_err()`，未断言状态码，所以这条规格偏差未被测到。 |
| F-13 | **MINOR** | `crates/platform-storage/src/http.rs:646-661` | JSON body 解析失败复用 `STORAGE_PRESIGN_INVALID` 作为 `code`，但 HTTP 状态是 **400**（而该码的固有状态是 403）。同一错误码在两种状态码下出现。 | 契约 C6 / D330 的码-状态一一对应；注释 `:650-652` 自认这是妥协（避免第 10 个码）。影响：前端按 `code` 取文案时，会把「body 不是合法 JSON」显示成「预签名令牌无效」。 |
| F-14 | **MINOR** | `crates/platform-storage/tests/storage_real_db.rs:554-650` | AC-S-183 步骤 1/2（`GET /api/storage/objects`、`GET /api/storage/jobs`）**只测了 service 层**（`service.list_objects` / `list_jobs`），没有一次 HTTP 请求；步骤 3 的上传会话表用 `TenantQuery` 直查（可接受）。另外 AC-S-183 前置条件要求「由 `scripts/seed-e2e.sh` 播种」，但该脚本**完全不涉及 storage**（`grep platform_storage scripts/` 零命中）。 | AC-S-183 步骤 1/2 的判据是 HTTP 端点的列表隔离。service 层能证明 RLS 生效，但**不能**证明路由/鉴权中间件没把租户串了。前置条件的播种脚本缺失 = 该 AC 无法按规格原样复现（测试自建数据替代）。 |
| F-15 | **MINOR** | `crates/platform-storage/src/http.rs:443-471` | 下载端点**完全不查元数据**：`verify_presign` 拿到 key 后直接 `objects().get(key)`，不检查 `platform_storage_object` 行是否存在、`status` 是否 `deleting`、也不复用对象的 `content_type`（与 F-06 同源）；错误响应用的 `RequestContext::new()`（**空租户**），因此该端点的 Problem Details 里没有真实 `tenantId`。 | 与 `get_object`（`service.rs:158-163`「先查元数据，同时起租户校验作用」）的纪律不一致。**更正说明**：我最初怀疑「DELETE 后的预签名仍能下载」，复核 `service.rs:169-178` 后**不成立**（`delete_object` 先删元数据行、再 `objects.delete` 删文件，两处都做）—— 该怀疑已撤回。剩余风险是「元数据已删但文件残留的孤儿对象仍可被旧 token 读到」，属条件性，未构造出复现。 |
| F-16 | **MAJOR** | `crates/platform-storage/tests/`（无任何 HTTP 测试） | 整个 crate **零 HTTP 层集成测试**：`grep oneshot / ServiceExt / axum::Router crates/platform-storage/tests/` 零命中；唯一涉及路由的测试是 `src/http.rs:688` 的 `the_router_registers_all_paths`（拼字符串看 `Debug` 输出）。 | AC-S-026（步骤 1~4 全是 HTTP：200 + url/expiresAt、逐字节下载、3 秒后 403）、AC-S-175 步骤 2~7（404/204/覆盖语义/400）、AC-S-179 步骤 2~3（415 状态码）、AC-S-183 步骤 1~2（列表端点）、AC-S-182 步骤 4（Content-Type）全部**没有一次真实请求**。这些判据在 service 层可能等价成立，但「HTTP 状态码/响应头/路由鉴权」这一层**完全未验证** —— F-06（Content-Type 错）与 F-12（应为 400 实际 404）正是因此漏网。 佐证：`crates/platform-storage/Cargo.toml:37-39` 声明了 dev-deps `tower` / `http-body-util` 并注释「测试要驱动 axum 路由（`ServiceExt::oneshot`）」，但全 crate **零使用**（`grep tower / http_body_util` 无命中）—— 说明 HTTP 测试计划存在但没写。 |
| F-17 | **MAJOR** | `crates/platform-storage/src/service.rs:366-373` | **分片上传完全绕过病毒扫描**：`complete_upload` 直接 `upsert_object`，`scan_verdict` 硬写成 `Skipped`，`scanner.scan()` 的调用点只有 `put_object`（`service.rs:125`）与导入（`worker.rs:143`）。 | D326 要求「`infected` → **拒绝落库** + 删除临时文件 + `STORAGE_FILE_INFECTED`」。分片是「大文件」的**唯一**上传通道（D321 的理由正是大文件），却恰恰没有扫描 → `maxUploadBytes` 内的任意大小恶意文件可经 `/uploads/*` 绕过 AC-S-178 步骤 4。AC-S-178 只用直传测，未覆盖分片。 |
| F-18 | **MINOR** | `crates/platform-storage/tests/storage_real_db.rs:339-444` | AC-S-177 的 4 条路径中 **步骤 7（abort 后 complete → 404）无服务层测试**（`grep abort crates/platform-storage/tests/` 零命中）；步骤 3（重传同片幂等 → 200）只在 `local_fs.rs:436` 的 store 单测里；步骤 4 的 `size == 3145728`（3 MiB / 1 MiB 片）被换成 **2560 字节**（`:63`）。 | AC-S-177 明确「主路径 + **5 条独立路径**」。尺寸替换是规模问题（可接受），但 abort→404 与「重传后 complete 成功 = 断点续传语义」这两条**判据本身没有服务层断言**。 |
| F-19 | **MINOR** | `crates/platform-storage/tests/storage_real_db.rs:211-230` | AC-S-178 步骤 2 判据 ② 写「断言 `.uploads/` 与**对象目录都为空**」，但测试改用文件名匹配，且同时要求阳性对照 `exact.bin` **存在** —— 两个要求互斥，「目录为空」这一字面判据不可能被断言；`.uploads/` 目录从未检查。 | AC-S-178 步骤 2 ②「拒绝发生在写盘之前，不是写完再删」。现有断言能证明「这次没写 over.bin」，但证明不了「没有临时残骸」，而规格特意点名了两个目录。 |
| F-20 | **MINOR** | `Cargo.toml:50` / `acceptance.md:59` vs `Cargo.lock` | 依赖破例的实际净增是 **5 个包**（`multer`、`encoding_rs`、`core_detect`、`multiversion_no_op`、`simdutf8`），不是规格与根 `Cargo.toml` 注释写的「**2 个包**」：`encoding_rs 0.8.44` 自身依赖 `core_detect`/`multiversion_no_op`/`simdutf8`（`Cargo.lock` encoding_rs 条目逐字可查）。`status.md` 的 rev 2 已改成 5，但 `acceptance.md` 与根 `Cargo.toml` 注释仍是 2。 | D321「净增 2 个包」的实测依据不成立（规格自身数字错误）。实现侧无夹带（新增 5 包全部来自 multer/encoding_rs 闭包），但**规格与代码注释都写错了数字** —— 本项目硬规矩「注释要与事实一致」。 |
| F-21 | **MAJOR** | `crates/platform-storage/src/object_store/s3.rs:471-494` | **恒真测试（假判据）**：`the_canonical_request_has_the_expected_shape` 在自己函数体里用 `format!` 拼出一个 canonical 字符串（还故意写成 host;host;x-amz-content-sha256;x-amz-date），然后断言这个**测试自己拼的**字面量 contains / starts_with —— **从头到尾没有调用被测实现**（`sign()` 一次都没调）。doc 却称它是「签名对但规范化错的**唯一**防线」。 | 正是规格反复警告的「判据恒真」类缺陷（S-03 / REQ-022）。改坏实现的规范化逻辑，这条测试**依然绿**。AC-S-175 的「SigV4 签名测试且通过」只剩一条自写 Python 的自洽比对（F-22），规范化拼接本身**无实现侧断言**。 |
| F-22 | **MINOR** | `crates/platform-storage/src/object_store/s3.rs:413-421` vs `decisions.md:54,58-62` | 规格声称「用 **AWS 官方测试向量** 验证 SigV4」，实现的测试 doc 却明确写「**为什么不用 AWS 官方向量**」，改用作者自写 Python 交叉验证（期望值 `b0e9826b…` 我独立复算**一致**，说明该自洽比对是真的）。但它的 canonical request 里签入了 `x-amz-content-sha256`，与 AWS 公开样例（只签 `host`+`x-amz-date`，期望 `5fa00fa3…`）**不是同一个输入**。 | D319 / design §1 的「有 AWS 官方测试向量」这一论据**不成立**；`status.md` 里的「SigV4 按官方向量验证通过」表述不准确。属实的是「与独立实现自洽」，不是「与官方向量一致」。 |

## 复核范围与结论

### 一、这次复核做了什么（方法与边界）

**没有跑 `cargo test`**（按任务约定）。所有结论来自：静态读代码 + `git diff`/`grep` + **独立复算**（SigV4 期望值用 Python 从零实现两套算法各算一遍，见 F-22）。测试条数用静态计数核对：`grep -cE '^\s*#\[(tokio::)?test\]'` = **141**（124 单测 + 4 契约 + 13 真库），与 BA 声明一致。

**实际读了**：
- 规格四件套：`acceptance.md`（全 452 行）、`decisions.md`（全 377 行）、`design-summary.md`（全 379 行）、`qa-req018-spec-review.md`（8 条，未重复报）
- `crates/platform-storage/src/`：`csv.rs`(全)、`worker.rs`(全)、`service.rs`(全)、`http.rs`(全)、`error.rs`(全)、`presign.rs`(全)、`local_fs.rs`(全)、`object_store/mod.rs`(全)、`authz.rs`(全)、`events.rs`(全)、`scanner.rs`(前半)、`config.rs`(前半)、`model.rs`(视图/JobView/DTO 段)、`store.rs`(claim_jobs / fail_job / 方法清单)、`s3.rs`(测试段 + 签名/规范化)
- 测试：`tests/storage_real_db.rs`(全)、`tests/contract_c5.rs`(全)、`tests/common/mod.rs`(fixture 能力清单)
- 其他：`manifest.json`、`Cargo.toml`(根 + crate)、`Cargo.lock` 与 `develop` 的差异、`migrations/2026112200*`、`capabilities/`(14 文件 / total 41)、`titles.rs`(TITLES=116)、`i18n_titles.rs`、`packages/i18n/{zh-CN,en-US}.json`、`packages/i18n/test/i18n.test.ts`、`scripts/`、`status.md`、`platform-server/src/main.rs`(storage 装配段)

**没读 / 没验（如实记账）**：
- `apps/shell/src/lib/storage-api.ts` 与其单测（前端交付，不在本轮 11 条 AC 的判据里，TD-063 已明码不做页面）
- `s3.rs` 的完整签名字符串拼接实现（只读了 `sign()` 签名与测试段）；因此「签名算法正确」我只能确认**与自写 Python 自洽**，不能确认「符合 AWS 全部规范」
- **没有跑**：全量 `cargo test`（1476 passed 声明）、4 个变异测试、`cargo xtask tenant-policies --check`、`capabilities --check`、3 个迁移的 up/down 往返 —— 这些**全部未独立复验**，按 BA 声明采信并标注
- **没有查库**（本机无 psql，用 podman 查询对结论无增量，故未做）

### 二、逐条 AC 结论

| AC | 结论 | 关键依据 |
|---|---|---|
| AC-S-026 预签名限时 | **不成立（判据未验证）** | 只有 `presign.rs` 纯函数单测；步骤 1~4 是 HTTP 行为（`{url,expiresAt}`、逐字节下载、过期 403），**零 HTTP 测试**（F-16）；下载端点 `Content-Type` 硬编码（F-06） |
| AC-S-175 trait + 本地 FS | **部分** | store 层 put/get/delete/覆盖/穿越都有单测；但步骤 7 要求 **400**，实现返 **404**（F-12）；HTTP 层零测试（F-16）；S3 测试含一条**恒真测试**（F-21） |
| AC-S-176 篡改防护 | **成立** | 4 条路径（改过期/改 key/改签名/过期但签名对）各自独立单测，且「签名先于过期」单独有测试（`presign.rs:340`）；错误码区分 `INVALID`/`EXPIRED` 正确 |
| AC-S-177 分片 | **部分** | 主路径/缺片/checksum 有真库测试；**abort→complete 404 无测试**（F-18）；重传幂等只在 store 单测；size 由 3145728 缩为 2560 |
| AC-S-178 大小上限 + 扫描 | **部分** | 边界/超限/`maxBytes`/默认 `skipped`/感染 422 都有断言；但 **② 判据被稀释**（F-19）、**分片通道完全绕过扫描**（F-17）、上传非流式（F-05） |
| AC-S-179 CSV + Excel 拒绝 | **不成立** | 步骤 1/2 成立；**步骤 4/5 要求「400 + 行号」，实现是异步 worker 里 `Err` → 任务 failed，没有 400**；步骤 3（`.xls`）与步骤 6（空文件 200）无集成测试（F-09） |
| AC-S-180 5 万行 + 进度 | **不成立** | 见 F-01/F-02：**目标表根本不存在、合法行从不入库**；测试换成 2500 行且**不查目标表** |
| AC-S-181 错误回执带行号列名 | **不成立** | 「第 3 行缺必填列」**无法产生逐行回执**（列数不符 → 整文件失败），`validate_row` 的缺列分支不可达（F-11）；行号在空行/多行字段下会漂移（F-10）；无该场景测试 |
| AC-S-182 导出 | **不成立** | `JobView` **无 `size`**（F-07）；下载 `Content-Type` 不是 `text/csv`（F-06）；round-trip 未覆盖「带换行字段」且只断「存在」（F-08） |
| AC-S-183 租户隔离 | **部分** | manifest 4 表**双向相等**断言 ✓（写得好）；4 表 RLS 真库各验一次 ✓；但步骤 1/2 是 service 层不是 HTTP（F-14），`seed-e2e.sh` 完全不播种 storage（F-14） |
| AC-S-184 权限/契约/审计 | **不成立** | 步骤 1a/1b/1c 全部成立（14 文件、total 41、TITLES 116、i18n 116、`all_codes` 含 storage）；但**步骤 5 的 4 个审计事件从未被发出**（F-03） |

### 三、最严重的三条

1. **F-01 + F-02（BLOCKER）**：导入**只数行不入库** —— `worker.rs:189-199` 的 `Ok(()) => {}` 空分支是全仓唯一的「合法行处理」，`entity` 参数只存进 job 表从不用于选表；`probe_rows` 表在迁移里**不存在**。AC-S-180 步骤 5（49990 行）与 AC-S-181 步骤 4（3 行入库）**没有实现**；测试把 5 万行换成 2500 行、并删掉了对目标表的所有断言 → 全绿。
2. **F-03（BLOCKER）**：4 个 `storage.*` 审计事件码**只有常量定义**，全仓没有任何发出点，`tests/storage_audit.rs` 不存在 → AC-S-184 步骤 5 整条未实现。
3. **F-17（MAJOR，安全）**：分片上传（大文件的唯一通道）`complete_upload` 不调扫描器、`scan_verdict` 直接写 `Skipped` → 配置了真扫描器时，恶意文件仍可经 `/uploads/*` 落库，绕开 AC-S-178 步骤 4。

### 四、做得好、不应被本轮结论掩盖的部分

- `presign.rs` 的签名/过期顺序、4 条篡改路径、常数时间比较、TTL 边界（0 / 上限 / 负值）都实现且测到位。
- `csv.rs` 的状态机对 BOM / 引号转义 / CRLF / 未闭合引号 / 单行超限的处理与单测是全 crate 质量最高的部分。
- `contract_c5.rs` 的 4 表**双向相等**、`all_codes()` 的**源码 grep 断言**（含阳性对照）、6 个配置键的 scope/default/lowerCamelCase 断言，都属于「有区分力」的判据。
- `local_fs.rs` 的 tmp+rename 原子写与并发同片测试、`is_safe_object_key` 的段级校验，符合设计。

### 五、总体判定

**FAIL。** 11 条 AC 的分布是：**成立 1 条**（176）、**部分 4 条**（175 / 177 / 178 / 183）、**不成立 6 条**（026 / 179 / 180 / 181 / 182 / 184）。不成立的 6 条里有 3 条是「实现根本没做」（180/181 的入库、184 步骤 5 的审计事件），而不是「做了但没测够」。`status.md` 的「11 条 AC 全达成」与「1476 passed」不能作为验收依据 —— 测试确实全绿，但绿的地方不覆盖那几条判据（F-02 正是把判据删掉才绿的）。

### 六、本轮**未验证**的事（不得当作通过）

- **测试命令硬规矩**：deliverable 里没有记录实际跑过的 `cargo test` 命令，因此「双 `--` / 漏 `--all-targets` / `--include-ignored` 位置」这三条无法复核。可确认的是：`crates/platform-storage/` 内**零 `#[ignore]`**（全仓的 `#[ignore]` 只在 `platform-app` 的既有真库用例里）。
- 全量 1476 passed、4 个变异确认变红、`tenant-policies --check` 退出码 0、3 个迁移 up/down 往返 —— 均按 BA 声明采信，**未独立复验**。
- 前端 `storage-api.ts` 与其单测未读（不在 11 条 AC 判据内）。

