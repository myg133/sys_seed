# REQ-018 修复后二次复核

> 复核者：独立 verifier（非实现者）
> 复核基线：feature/REQ-018 @ `4043aa1`
> 状态：**已完成** —— 结论 **仍有 MAJOR**（3 条 BLOCKER 全部真修；9 条声明已修 = 8 真修 + 1 部分；无 BLOCKER 复发）

## §0 复核过程中的基线漂移（必读）

复核开始 HEAD = `62cbdef`（clean）。复核期间开发者**持续提交**，HEAD 两次前进：

| 时点 | HEAD | 变化 |
|---|---|---|
| 复核开始 | `62cbdef` | 声明 9 条已修（F-10 未修） |
| 中途 18:51 | `4043aa1` | 修 F-10（rowNumber 用解析器行号） |
| 复核末 19:0x | `de5be5a` | 修 F-18 + 测试基础设施竞态（独立租户） |
| 复核结束时**未提交** | 工作区 | 在途改 `s3.rs`（修 F-21）+ `storage_real_db.rs` |

**这对结论的影响（逐条说明）**：

- **F-10**：`62cbdef` 未修 → `4043aa1` **修了一半**（空行已修、引号内换行仍错）。
- **F-21**：`de5be5a` 及之前**只剩注释**（R-01 成立）；但复核结束时工作区有**在途未提交**改动，
  抽出 `canonical_request()` 并补了**真判据**（逐段断言 + 负面对照 `!contains("host;host")`）——
  该修复**在此刻尚未提交、未跑过测试**，故我**不能**判它已修（见 R-07）。
- **R-02（夹具竞态）**：`de5be5a` 声明修了此项。我**实测验证过**（见 R-08）：
  修法方向是对的（锁覆盖清表 + 清表带 `WHERE tenant_id=$1` + `OnceLock`），
  但 `--all-targets` 下仍有 1~3 条随机失败 → **未完全修好**。
- **R-05（entity 静默忽略）**：复核末工作区已**实现** entity 白名单校验
  （`KNOWN_ENTITIES = ["probe_row"]`，`service.rs:685-696`，未知 → 400）。
  第一次看到它时**编译不过**（`error.rs:73` 尚无变体），约 2 分钟后补齐可编译。
  因**未提交**，我判「`de5be5a` 上未修 + 已在修」，**不采信为已修**。
- ⚠️ **重要**：19:09 工作区曾**不可编译**（`UnknownEntity` 未补齐）。我在此之前的测试结论有效；
  开发者随后补齐后，我又跑了一组对照实验（见 R-08 的「4 失败 vs 9 全绿」）。

下文所有 `file:line` 以 `4043aa1` 为基准；`de5be5a` 之后的差异单独标注。

另检测到**另一个 agent 并发跑 `storage_real_db`**（18:53 `ps aux` 可见）。
本仓库夹具 `setup()`（`tests/common/mod.rs:333-342`）在**释放 advisory lock 之后**才
`DELETE FROM` 5 张 storage 表 → 两个进程互相清表。我因此观测到多次假失败
（`ObjectNotFound`、`上传会话表：只应看到 1 行 left:0`）。
**停掉并发后串行连跑 3 次 = 18/18 全绿** → 判定为**夹具隔离缺陷**，非产品缺陷（R-02）。

## 逐条 FINDING 复验

| 原 FINDING | 严重级 | 声明已修 | 我的复验结论 | 证据 |
|---|---|---|---|---|
| F-01 合法行入库 | BLOCKER | 是 | **真修** | `store.rs:511-590` `insert_rows_batch`；`worker.rs:214-221`/`:243-249` 真实调用；`count_probe_rows` `store.rs:584` |
| F-02 5 万行 + 目标表断言 | BLOCKER | 是 | **真修** | `storage_real_db.rs:349` `DATA_ROWS=50_000`；`:441-449` 断言 49990；实测 9.0s 通过 |
| F-03 审计四处埋点 | BLOCKER | 是 | **真修** | 4 处：`service.rs:227-239`/`:272-279`、`worker.rs:271-282`/`:346-357`；`storage_audit.rs:129-167` 差值+双向相等；连跑 2 次绿 |
| F-04 partSizeBytes 硬上限 | MAJOR | 是 | **真修** | `service.rs:364-387`；`:830-897` 测 `=1024` 过 / `1025` 拒 / `0`、`-1` 拒；实测绿 |
| F-06 下载 Content-Type | MAJOR | 是 | **真修** | `http.rs:473-491` 白名单**仅 csv**，`.html/.svg/.js` 一律 octet-stream |
| F-07 JobView.size | MAJOR | 是 | **真修** | `model.rs:301`；`http.rs:641-646` 取**对象元数据** size_bytes（非行数）；`storage_http.rs:498-501` 断言 `size>0` |
| F-11 validate_row | MAJOR | 是 | **实现真修 / 判据不完整** | `worker.rs:389-450` 改判「必填列单元格为空」；但 AC-S-181 端到端用例缺失 → R-03 |
| F-14 seed-e2e 播种 | MINOR | 是 | **真修（有 3 处保留）** | `seed-e2e.sh:606-641` 2 租户；`:655-676` 强断言；但漏第 5 表 → R-04 |
| F-16 HTTP 测试 8 条 | MAJOR | 是 | **真修** | `storage_http.rs` 8 条全 `oneshot`；实测 8/8 绿 |
| F-17 分片扫描 | MAJOR | 是 | **真修** | `service.rs:494-521` 扫描在落库前、感染删文件 + 标 `aborted`；`:910-977` 测试 |
| F-21 s3 恒真测试 | MAJOR | 删了，未补 | **部分（仍缺真判据）** | `s3.rs:466-473` 只剩注释 → R-01 |
| F-12 路径穿越 404/400 | MINOR | 未声明 | **如实记录偏差（可接受）** | `storage_http.rs:555-566` 显式 `assert_ne!(status, 400)` 记偏差 |

## §9 条声明已修的实际结论

**真修 8 条**：F-01、F-02、F-03、F-04、F-06、F-07、F-14、F-16。**部分 1 条**：F-21。

### F-01 的内核核对（不是看有没有函数，是看 SQL 对不对）

- `{tenant}` **恰好一次**：`store.rs:566-573` 里只在 `SELECT u.id::uuid, {tenant}, ...` 出现一次
  → `TenantQuery::render`（`query.rs:251-291`）的「恰好一个」硬约束**未被破坏**。
- 参数类型：`BindValue::TextArray(Vec<String>)`（`query.rs:46`、`:67`）**真实存在**；
  SQL 用 `?::text[]` + `u.amount::bigint` 转换（注释 `store.rs:551-556` 说明为何走 text[]）—— 与代码一致。
- `ON CONFLICT (tenant_id, job_id, email) DO NOTHING` 与迁移唯一约束
  `platform_storage_probe_row_job_email_key`（`migrations/...00400.up.sql:50`）**列顺序完全一致**。
- 判据区分力：`count_probe_rows` 是 `SELECT count(*) ... WHERE job_id=?`（`store.rs:584-599`），
  直接数**目标表**，与 `processed_rows`（任务表计数）是**两个不同的表** → 能区分「数了行」与「入了库」。**
  实测：把入库路径删掉则 49990 断言必红（`processed_rows` 与 `count_probe_rows` 断言分别独立存在）。

### F-03 的判据形态（重点怀疑方向 1）

- **是差值断言，不是绝对值**：`storage_audit.rs:65` 取 `baseline`，`:133-136` 算 `delta` —— 正确。
- **幂等已实测**：连跑 2 次均 2 passed（`the_four_storage_events_reach_the_audit_log` /
  `storage_still_works_without_an_audit_sink`）。原因是 `:33` 用**专属租户** `tenant-req018-audit`。
- **4 个事件码全验**：`:140-145` 显式列 4 条期望，`:164-167` 集合**双向相等**（防第 5 个混入）。
- 生产装配也真做了：`platform-server/src/main.rs:1132-1137` 装配失败直接 `Err` 启动失败
  → 不是「测试里装了、生产没装」的假绿。

### F-01 / F-02 的**直接数据库证据**（最强证据）

不只靠测试自报。跑完 `a_large_import_reports_progress_and_row_level_errors`（1 passed, 11.53s）
后立刻按 job 分组查库：

```
$ podman exec base-postgres psql -U base -d base_test -tAc \
    "select job_id, count(*) from platform_storage_probe_row group by job_id order by 2 desc;"
03943f39-4fbd-4901-b6a3-a4048c262d21|49990     ← 该用例的 job，恰好 49990
06e7123f-...|1     （其余 1 行/条的是其他用例的）
...（共 8 条 1 行的）
```

→ **目标表里确实有 49990 行**，且按 job 归属正确。
这不是「数了行」：`processed_rows` 在 `platform_storage_job`，而这里数的是
`platform_storage_probe_row` —— 两张不同的表，只有真的 INSERT 才会有行。
**F-01 / F-02 判定为真修，证据充分。**

（总行数 49998 = 49990 + 8 条其他用例遗留；因另一 agent 正循环跑全 workspace
`cargo test -p <crate> --all-targets -- --test-threads=1`，清表与我交错。）

## 新发现的问题

### R-01（MAJOR）F-21 只删了恒真测试，没有补真判据

`s3.rs:466-473` 现在是**纯注释**。注释声称「把 `sign()` 改成 `unimplemented!()` 它会变红」——
这一句**成立**（`the_signature_matches_an_independent_python_implementation` `:443-464` 真调 `sign()`），
所以**签名链本身有判据**、不再是恒真测试。

但原 F-21 的核心指控是「canonical request 的**规范化拼接**无实现侧断言」——
这一点**仍然成立**：`sign()` 把 canonical request **内联拼接**（无独立函数可测），
唯一的端到端断言只比对**最终签名值**。若规范化里的分隔符/头列表写错，
最终签名会变 → 会被抓到；**但**若实现与 Python 脚本**错得一样**（比如都漏了
`x-amz-content-sha256` 进入 signed_headers），断言仍绿。
→ 结论：**从「恒真」升级为「自洽比对」，但「规范化拼接正确」仍无独立判据**。降级为 MAJOR（原为 MAJOR，未消除）。

### R-02（MINOR）夹具清表在 advisory lock 之外 → 跨二进制并发必假失败

`tests/common/mod.rs:288-325` 拿锁做迁移/RLS，`:327` **释放锁**，`:333-342` 才 `DELETE FROM`。
两个测试二进制（`storage_http` / `storage_real_db` / `storage_audit`）**并行**跑时
（cargo 默认行为，或 `--all-targets` 会跑所有二进制）会互相清表。
**实测证据**：`--all-targets` 下 `storage_http` 8 条里 7 条失败，失败信息是
`左 404 右 200`、`the object was not found: docs/a.txt` —— 对象刚 PUT 就被别的进程 `DELETE`。
单跑 + 串行连跑 3 次 = 全部绿。
→ 不影响 CI 若固定 `--test-threads=1` 且不并发多二进制，但**测试基础设施不可靠**。

### R-03（MAJOR）AC-S-181 的端到端场景仍无用例（F-11 只修了**能力**，没补**判据**）

F-11 的实现确实改对了：`worker.rs:412-421` 判「必填列单元格为空」→ `column: "email"`，
不可达的「列名在 map 里」分支已删。单测 `:558-568` 覆盖「空 email → column=email」。

但规格 AC-S-181（`acceptance.md:320-338`）的核心判据是**一个 5 行 CSV 的完整场景**：
> 第 3 行缺 `email`、第 5 行 `amount` 非法 → **恰好 2 条**回执（`{3,"email"}`、`{5,"amount"}`）
> + **只有 3 行入库**（第 1、2、4 行）。

`grep -rn "AC-S-181" tests/` 只命中两处**注释**（`storage_real_db.rs:353`、`:471`），
`s12grep ",,"` 无命中 → **这个 5 行场景一次都没跑过**。
现有测试是分散的：`a_large_import...`（10 条 amount 错、无 email 空）、
`row_numbers_stay_correct_when_the_file_contains_blank_lines`（1 条 amount 错、无 email 空）、
`a_clean_import_has_no_error_receipts`（0 条错）。
→ **「email 空」这条路径在集成层从未与「入库 3 行」一起被断言**。

### R-04（MINOR）seed-e2e 只播 4 张表，漏了 `platform_storage_upload`，且断言是 `< 8`

- `seed-e2e.sh:606-641` 只 INSERT 了 object / job / job_error / probe_row **4 张**；
  `grep platform_storage_upload scripts/seed-e2e.sh` → 零命中。
- 但 AC-S-183 步骤 3 要求 5 张表都验隔离 → **upload 表在 e2e 夹具里是空集合**，
  「只返回本租户的行」在空集合上 trivially 成立 = 假绿（正是脚本自己注释要防的形态）。
- 断言写的是 `-lt 8`（`< 8` 才失败，`:672`）→ 实际允许 8/9/10 任意值通过，
  与注释「**恰好** 1 行/租户」不符（注释 `:660` 说「恰好 1 行」）。
- 加载点：`seed-e2e.sh:21` 的 `storage_fixture` 默认 `""`，若 `run_sql` 失败会退化成 `0` → 触发 `-lt 8` 失败，尚可。

### R-05（MAJOR，复核末已在修）`entity` 曾是自由字符串：任意 entity 静默写入 `probe_row`

**在 `de5be5a` 上成立**：

- `service.rs:593`/`:607` 直接 `entity: body.entity.clone()`，**零校验**；
- `worker.rs` 完全不读 `job.entity` 决定目标表 —— `export_rows`/`run_import` 恒用
  `PROBE_ROW_TABLE`（写死）。
- 后果：`entity: "users"` / `"orders"` / 拼错成 `"probe_rows"`（**规格原文就是复数**，
  `acceptance.md:291`）全部**静默写入 `platform_storage_probe_row`**，
  接口返回 202、任务 `completed`、无任何错误。
- 「参数存在但被静默忽略」与本项目「禁用不报错的静默行为」相悖。

**复核末（19:11）开发者已实现修复**（工作区未提交）：`service.rs:685-696`

```rust
pub const KNOWN_ENTITIES: &[&str] = &["probe_row"];
pub fn ensure_known_entity(entity: &str) -> Result<(), StorageError> { ... }
```

`UnknownEntity` → `400 BAD_REQUEST`（`error.rs:178`），detail 里列出已知实体
（`error.rs:212`）。并新增 HTTP 用例 `an_unknown_entity_is_rejected_when_the_job_is_created`
（`storage_http` 从 8 条变 9 条，我实测该条存在且串行通过）。

→ **方向正确**。但因未提交、且工作区当时不可编译，我**不判为已修**。

### R-06（已闭环，保留作记录）5 万行测试的 doc 注释曾与代码矛盾

**在 `de5be5a` 上成立**：`storage_real_db.rs:336-338` 的 doc 仍写
「⚠️ 这里用 **2500 行**…而不是 5 万 —— 5 万行会让这条用例在低配机上跑几分钟」，
而函数体 `:349` 是 `const DATA_ROWS: usize = 50_000`；实测该用例 **9.03 秒**
（不是「几分钟」）。同一文件 `:353` 又写「规模按规格来」→ 两条注释互相打架。
这正是 F-02 的**原文残留**（补了代码、没删旧注释）。

**复核末（19:11）开发者已修**：现在 `:334-336` 是

> **规模就是规格要求的 50001 行**（1 表头 + 5 万数据行），不缩水。
> 实测耗时约 9 秒（低配机上可接受），所以不存在「缩规模」的理由。

与代码、与实测**三者一致**。→ **本条已闭环**（但该修复在工作区，未提交）。

### R-07（流程，非代码缺陷）F-21 的修复在复核结束时**尚未提交**

复核末工作区未提交 diff（`crates/platform-storage/src/object_store/s3.rs`）显示开发者正在修 F-21：

- 抽出 `pub fn canonical_request(input) -> String`（原先内联在 `sign()` 里）；
- 新增 `the_canonical_request_is_assembled_in_the_aws_prescribed_shape`，
  **调用被测函数**并逐段断言 9 段结构（方法/URI/查询/3 个头/空行/清单/负载哈希）+
  负面对照 `!canonical.contains("host;host")`。

**这个方向是对的**（真正调用被测代码，不再是自拼字面量）。但：

- 它**未提交**（`git status` 显示 ` M`），也**未跑过测试**；
- 因此我的 F-21 结论只能是「`4043aa1`/`de5be5a` 上**仍未修**（R-01 成立）」，
  加上「复核末已在修且方向正确」。
- ⚠️ 若之后提交，需**重新复核**：重点看 `canonical_request` 是否真被 `sign()` 调用
  （否则又成了两套并行的假判据）、以及逐段断言是否与 AWS 规范一致
  （注意：该实现把 `x-amz-content-sha256` 纳入 signed headers，与 AWS 公开样例
  `5fa00fa3…` 不是同一输入 —— F-22 的问题不因这条测试而消失）。

### R-08（MAJOR）`de5be5a` 声称修了夹具竞态（「连跑 3 次 158/0，稳定」）—— **实测未达标**

**提交说明的修法**（`git show de5be5a`）三条根因 + 三条修复，方向**全部正确**：

| 根因 | 修法 | 我的核验 |
|---|---|---|
| 清表在 advisory lock 之外 | 锁持有到清表+配置重置后（`common/mod.rs:415-425`） | ✅ 代码确实把 `unlock` 移到清表之后 |
| 4 个二进制共享硬编码 `tenant-a` | 每个二进制用 `tenant-a@<CARGO_CRATE_NAME>`（`:31-50`） | ✅ `ctx_a()` 走 `tenant_a()`（`:253-258`） |
| 迁移/策略并发互相干扰 | 进程内 `OnceLock` 只做一次（`:82-86`） | ✅ `DEPLOYED.get()/set()` |
| 清表无条件 `DELETE FROM` | 改成 `WHERE tenant_id = $1`（`:392-401`） | ✅ 逐租户删除 |

**但实测结论不成立**（无任何并发进程时）：

```
$ cargo test -p platform-storage --all-targets     # 3 次，每次失败点不同
第 1 次: storage_http FAILED 5 passed / 3 failed
第 2 次: storage_http FAILED 7 passed / 1 failed
          panicked: ttl=0 必须 400   left: 404  right: 400
```

- 失败信息是「对象刚 PUT 就 404」（`ttl=0 必须 400, left: 404`）→ **仍是数据被抹掉**的形态。
- **决定性对照实验**（同一二进制，只改 `--test-threads`）：

```
$ cargo test -p platform-storage --test storage_http            # 默认 8 线程并行
FAILED: an_unknown_entity_is_rejected_when_the_job_is_created
FAILED: a_tampered_token_is_invalid_not_missing
FAILED: an_expired_token_is_rejected_with_the_expired_code
...（4 条失败）

$ cargo test -p platform-storage --test storage_http -- --test-threads=1
test result: ok. 9 passed; 0 failed      # 全绿
```

→ **同一个二进制、同一份代码，只因为「并行 vs 串行」就从 4 红变 0 红。**
这**排除了**产品缺陷，把根因锁死在测试隔离（根因 a）。
- 串行 `--test storage_real_db` = 18/18 全绿。
- → `de5be5a` 声称的「连跑 3 次 158/0，稳定」**与我的实测不符**。
  **至少 `storage_http` 与 `storage_real_db` 并行时仍互相干扰。**

**⚠️ 已定位到根因（两个叠加，这是本报告给开发者的最有价值线索）：**

**(a) 同一测试二进制内的用例共享同一个租户**（最主要）

`storage_http.rs` 的 7 条用例**全部** `let ctx = fixture.ctx_a()`
（`:164 :220 :264 :301 :341 :396 :443`），而 `ctx_a()` 返回的是
`tenant-a@storage_http` —— **同一个租户**。cargo 默认按 `nproc=8` **并行**跑同一
二进制内的用例 → 用例 A 的 `setup()` 执行 `DELETE FROM ... WHERE tenant_id='tenant-a@storage_http'`，
**正好清掉正在并行跑的用例 B 刚 PUT 进去的对象** → B 的断言得到 `404`。
这完美解释「单跑绿、全跑随机红、换用例换失败点」。

→ `de5be5a` 的「每二进制独立租户」只解决了**跨二进制**污染，**没解决跨用例**污染。
修法：`setup()` 的租户再叠一层**用例级**唯一后缀（如 `tenant-a@storage_http#<uuid>`），
或给 `setup()` 加进程级互斥（`tokio::sync::Mutex`）让同二进制内 setup 串行。

**(b) 6 个配置键是全库共享的，不随租户/二进制隔离**（叠加因素）

`common/mod.rs:404-426` 的「4b) 重置配置」在**锁内**执行（顺序没问题），但它清的是
`platform_config` 里的 `storage.maxUploadBytes` / `storage.partSizeBytes` /
`storage.presignTtlSeconds` / `storage.importBatchSize` / `storage.maxRowBytes` /
`storage.uploadSessionTtlSeconds` —— 这 6 个键是 **platform scope**（manifest 里
`"scope": "platform"`），**全库一份，不按租户隔离**。

于是：

```
时刻 T1  二进制A( storage_http )  setup(): 持锁 → 重置 6 键为默认 → 解锁
时刻 T2  二进制A 的用例把 presignTtlSeconds 改成 2、把对象 PUT 进去
时刻 T3  二进制B( storage_real_db ) setup(): 持锁 → **又把 6 键重置为默认** → 解锁
时刻 T4  二进制A 继续断言 → 配置已被 B 改掉 → 行为不符预期
```

**这与观测到的失败形态吻合**：`ttl=0 必须 400` 得到 `404`（配置被改 → TTL 校验分支变化）、
`an_export_download_is_served_as_text_csv` 得到 `404`（对象被清）、
「换一条用例就换一个失败点」。

→ **仅靠「每二进制独立租户 + 清表带租户条件」修不掉这一类失败**，
因为**配置不在租户维度**。真正的修法二选一：
(a) 配置源改成**进程内可注入**（`StorageConfigSource` 已经是 trait，测试用内存实现，
   不落 `platform_config` 表）；或
(b) 6 个键的读写也纳入 advisory lock，并在**整个用例生命周期**内持锁（代价：完全串行）。

→ **R-08 判定：`de5be5a` 的修复不充分，「158/0 稳定」的声明不接受。**

## 22 条 FINDING 的最终状态

| # | 级别 | 状态 | 说明 |
|---|---|---|---|
| F-01 | BLOCKER | **已修** | 见上 |
| F-02 | BLOCKER | **已修** | 见上 |
| F-03 | BLOCKER | **已修** | 见上 |
| F-04 | MAJOR | **已修** | 硬上限，边界可翻转 |
| F-05 | MAJOR | **未修** | `http.rs:368`/`:524` 仍 `to_bytes(256MiB)`；`:335` `field.bytes()`。非流式 |
| F-06 | MAJOR | **已修** | 白名单仅 csv |
| F-07 | MAJOR | **已修** | size 取对象元数据 |
| F-08 | MAJOR | **未修** | `storage_real_db.rs:610/614/618/628` 仍 `iter().any(...)`「存在」断言，未逐字逐行 |
| F-09 | MAJOR | **部分修** | `.xls` 已由 `storage_http.rs:390-435` 覆盖（415）；空文件 200 仍无集成用例 |
| F-10 | MAJOR | **半修** | 空行半修（未提交→已提交 `csv.rs` `record_lines`）；**引号内换行仍不递增 `line`**（`csv.rs:151-168` 的 `in_quotes` 分支无 `line += 1`） |
| F-11 | MAJOR | **已修（实现）/ 缺判据** | 见 R-03 |
| F-12 | MINOR | **未修（已如实记录）** | 仍 404，测试显式记偏差 |
| F-13 | MINOR | **未修** | 未声明修 |
| F-14 | MINOR | **已修（有保留）** | 见 R-04 |
| F-15 | MINOR | **未修** | `http.rs:443-471` 仍不查元数据；错误用 `RequestContext::new()` 空租户 |
| F-16 | MAJOR | **已修** | 8 条 HTTP |
| F-17 | MAJOR | **已修** | 分片扫描 |
| F-18 | MINOR | **已修** | `storage_real_db.rs:1119` abort→complete 404 |
| F-19 | MINOR | **未修** | `.uploads/` 仍未检查；仍只断「无 over.bin」+阳性对照 |
| F-20 | MINOR | **已修（注释）** | `Cargo.toml:50,53` 已改「净增 5 个包」并在注释说明原文错误 |
| F-21 | MAJOR | **部分** | 见 R-01 |
| F-22 | MINOR | **未修（且自相矛盾）** | `s3.rs:7`/`:12` 称「按 AWS 官方向量」，`:413` 称「为什么不用官方向量」 |

**统计**：已修 12 条（BLOCKER 3 全修 + MAJOR 6 + MINOR 3）；部分 3 条（F-09 / F-10 / F-21）；
未修 7 条（F-05 / F-08 / F-13 / F-15 / F-19 / F-22 及 F-12 的规格偏差）。

## 我实际执行过的命令（可复现）

```bash
export CARGO_TARGET_DIR=$PWD/target
export DATABASE_URL_TEST="postgres://base:base@127.0.0.1:5432/base_test"

# 审计（连跑 2 次验幂等）→ 均 2 passed
cargo test -p platform-storage --test storage_audit --all-targets -- --test-threads=1

# 5 万行导入（无并发时）→ 1 passed, 9.03s
cargo test -p platform-storage --test storage_real_db a_large_import -- --test-threads=1

# partSize 硬上限 → 1 passed
cargo test -p platform-storage --test storage_real_db the_part_size_config -- --test-threads=1

# HTTP 8 条 → 8 passed
cargo test -p platform-storage --test storage_http -- --test-threads=1

# 契约 → 4 passed
cargo test -p platform-storage --test contract_c5 -- --test-threads=1

# 真库全量串行 × 3 → 均 18 passed
cargo test -p platform-storage --test storage_real_db -- --test-threads=1

# 租户策略自检 → 「通过：7 个模块」
DATABASE_URL=... cargo run -q -p xtask -- tenant-policies --check
```

**未执行**（如实记账）：全量 `cargo test` 整个 workspace（另一 agent 在跑）、
`capabilities --check`、3 个迁移 up/down 往返、前端 `storage-api.ts`。

## 总体判定

**仍有 MAJOR（无 BLOCKER 复发）。**

### 一句话结论

上一轮的 3 条 BLOCKER（F-01 只数行不入库 / F-03 零审计埋点 / F-02 判据被删）
**全部真修，且我用数据库直查验证了 49990 行真实落库** —— 这是本轮最重要的正面结论。
但仍有 MAJOR 未清，其中 **R-08（测试基础设施仍不稳定）** 是最需要注意的一条：
它会让「全量测试全绿」这个验收依据本身不可信。

### 9 条声明已修的判定

**真修 8 条**：F-01、F-02、F-03、F-04、F-06、F-07、F-14、F-16  
**部分 1 条**：F-21（`de5be5a` 上只剩注释；复核末在途修复方向正确但未提交）  
**实现修好但判据不全 1 条**：F-11（见 R-03）

### 22 条 FINDING 的最终账

| 状态 | 条数 | 明细 |
|---|---|---|
| **已修** | 11 | F-01 F-02 F-03 F-04 F-06 F-07 F-14 F-16 F-18 F-20 +（F-12 如实记录偏差，可接受） |
| **部分** | 4 | F-09（缺空文件 200 用例）、F-10（空行已修 / 引号内换行仍错）、F-11（实现对 / 缺 AC-S-181 端到端）、F-21（只删未补，在途） |
| **未修** | 7 | F-05 非流式、F-08 未逐字相等、F-13 码-状态错配、F-15 不查元数据、F-19 未查 `.uploads/`、F-22 注释自相矛盾 |

> 注：F-12 计入「已修」是因为测试**如实记录了 404 与规格 400 的偏差**（`assert_ne!(status, 400)`），
> 达到「不得隐瞒偏差」的要求；规格本身未变，故仍记 TD。

### 本轮新增问题（按严重度）

| # | 级别 | 一句话 |
|---|---|---|
| **R-08** | **MAJOR** | `de5be5a` 称「158/0 稳定」，实测 `--all-targets` 仍随机失败 1~3 条；根因是 **6 个 `scope=platform` 配置键全库共享**，租户隔离修不掉 |
| **R-05** | **MAJOR** | `de5be5a` 上成立（任意 entity 静默写 `probe_row`）。复核末已实现 `ensure_known_entity` 白名单 + 400 + 新用例（未提交，故不判已修） |
| **R-03** | **MAJOR** | AC-S-181 的「2 条回执 + 3 行入库」5 行场景**一次都没跑过**（只有能力、没有判据） |
| **R-01** | **MAJOR** | F-21 只删了恒真测试；canonical 规范化仍无独立实现侧判据（在途修复未提交） |
| **R-07** | 流程 | F-21 修复在复核末未提交；若提交需重验（尤其 `canonical_request` 是否真被 `sign()` 调用） |
| **R-04** | MINOR | seed-e2e 漏播 `platform_storage_upload`（5 表只播 4 张）+ 断言用 `-lt 8` 而非「恰好」 |
| **R-06** | MINOR | 曾：doc 注释写「2500 行」而代码是 `50_000`。**复核末已修**（注释改为「就是 50001 行，实测 9 秒」）→ 已闭环 |
| **R-02** | MINOR | （已被 R-08 吸收，保留作历史记录）清表曾在锁外 |

### 建议下一步（按优先级）

1. **修 R-08 的根因**：把 6 个 `platform` scope 配置键的测试读写改成进程内注入
   （`StorageConfigSource` 已是 trait，不必落表），或整个用例生命周期持 advisory lock。
   **在修好之前，任何「全量 158/0」的声明都不能作为验收依据。**
2. **R-05**：已在工作区实现（白名单 + 400），**提交后重验**即可（重点看 `create_import` 与 `create_export` 两处都调了校验）。
3. **补 R-03**：按 AC-S-181 原文写那个 5 行 CSV 的端到端用例（2 条回执 + 3 行入库 + 行号 3/5）。
4. **修 F-05 / F-08**：上传改流式（不用 `to_bytes`）；round-trip 改逐字逐行断言。
5. **修 F-22**：`s3.rs:7/:12` 与 `:413` 的注释必有一处要改（本项目「注释必须与事实一致」）。

### 本轮**未验证**的事（不得当作通过）

- 全量 workspace `cargo test`（另一 agent 在跑；且复核末工作区**不可编译**）
- `cargo xtask capabilities --check`、3 个迁移 up/down 往返
- 前端 `apps/shell/src/lib/storage-api.ts` 与其单测（不在 11 条 AC 判据内）
- 复核末在途未提交改动（`s3.rs` 的 F-21 修复、`service.rs` 的 `UnknownEntity`）—— **一律不采信**

---

*复核者签名：独立 verifier（未修改任何代码/测试/规格；仅写入本文件）*
*复核时间：2026-11-22 18:47 ~ 19:12 CST*
