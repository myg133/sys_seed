# 05 权限点、C-5 契约、启动对账、审计埋点（AC-S-174）

> 测试落点：`crates/platform-notify/tests/contract_c5.rs`（**本需求新增**）+
> `crates/platform-server/tests/i18n_titles.rs`（**既有测试，必须继续绿**）+
> `crates/platform-notify/tests/notify_audit.rs`（design §9）。
> 步骤 3 / 4 是**阳性对照**（故意把东西弄坏，断言测试会红）—— 它们证明 1b 的绿不是假绿。

## 步骤 1a：capabilities 重新生成

### TC-015-078 `capabilities/` 下恰好 14 个文件（不增不减）
- **AC**: AC-S-174 步骤 1a ｜ **类型**: 可维护性 ｜ **优先级**: P0 ｜ **自动化**: 集成（命令 + 文件计数）｜ **方法**: 边界值
- **前置**: 工作树干净；`cargo xtask capabilities` 可跑（**本用例执行时才跑，QA 验收前不预跑**）
- **步骤**: 1) 跑 `cargo xtask capabilities` 重新生成 2) 统计 `capabilities/` 下文件总数（递归）3) 单独统计 `capabilities/sdk/` 下文件数
- **预期**: ① 文件总数**恰好 `14`**（逐字 14，不是 ≥14）；② 其中**固定产出 7 个**：`permissions` / `extension-points` / `schema/manifest` / `schema/metadata` / `schema/entity` / `api` / `index`（`crates/xtask/src/capabilities.rs:54-81` 的 `generate`）；③ `capabilities/sdk/` 下**恰好 7 个**（`auth` / `hooks` / `i18n` / `schema` / `sdk` / `theme` / `ui`）；④ 关键判据：**新增 Rust crate 只让 `permissions.json` 多一个 module 条目，不新增文件** —— 若实现顺手加了 `capabilities/notify.json`，总数会是 15 → 红

### TC-015-079 `permissions.json` 的 `total == 38` 且含 3 个 notify 权限点逐字
- **AC**: AC-S-174 步骤 1a 续 ｜ **类型**: 可维护性 ｜ **优先级**: P0 ｜ **自动化**: 集成（读 JSON）｜ **方法**: 边界值
- **前置**: TC-015-078 的生成已完成
- **步骤**: 1) 读 `capabilities/permissions.json` 的 `total` 字段 2) 读 `modules` 数组里 `crates/platform-notify/manifest.json` 那一项 3) 读其权限点编码列表
- **预期**: ① `total` **恰好 `38`**（实测基线 35 = 6+3+3+4+6+9+4，**+3** = 38）；② `modules` 里**新增恰好 1 个**条目，其路径**逐字** `crates/platform-notify/manifest.json`；③ 该条目的权限点编码**逐字**含且仅含 `notify:template:read` / `notify:template:write` / `notify:delivery:read` **3 条**；④ 这 3 条的 `type` 分别为 `menu` / `button` / `menu`（design §6 的 manifest 原文）；⑤ 负向断言：`total` **不是 35**（未生成）、**不是 39+**（多登记了权限点）

## 步骤 1b：C-5 契约测试通过

### TC-015-080 `cargo test -p platform-notify --test contract_c5` 通过
- **AC**: AC-S-174 步骤 1b ｜ **类型**: 契约 ｜ **优先级**: P0 ｜ **自动化**: 契约（`contract_c5`）｜ **方法**: 等价类
- **前置**: `crates/platform-notify/tests/contract_c5.rs` 由本需求新增并存在
- **步骤**: 跑 `cargo test -p platform-notify --test contract_c5`
- **预期**: ① 退出码 **0**；② 输出含 `test result: ok`，**`0 failed`**；③ 测试文件**存在**（若不存在，命令会报 `no test target named contract_c5` → 红 —— 这与「测试没跑也算过」区分开）；④ 该测试覆盖 C-5 的两条：每个 manifest 权限点都有 `require_permission` 字面量调用点、且**恰好一个**鉴权入口

## 步骤 1c：i18n_titles 既有测试继续绿（两条硬编码必须同步更新）

### TC-015-081 `i18n_titles` 通过，且行数钉死断言已加 `REQ_015_NEW_CODES`
- **AC**: AC-S-174 步骤 1c ｜ **类型**: 契约 ｜ **优先级**: P0 ｜ **自动化**: 集成（既有测试）｜ **方法**: 边界值
- **前置**: `crates/platform-server/tests/i18n_titles.rs` 存在（既有）
- **步骤**: 1) 跑 `cargo test -p platform-server --test i18n_titles` 2) 读源码中 `the_title_table_has_the_pinned_number_of_rows`（`crates/platform-server/tests/i18n_titles.rs:174`）的常量定义
- **预期**: ① 测试通过，`0 failed`；② 源码里**存在** `const REQ_015_NEW_CODES: usize = 8;`（**逐字 8**）；③ 该 assert 的算式包含这一项（`58 + REQ_013(21) + REQ_014(5) + REQ_022(9) + REQ_016(6) + REQ_015(8) = 107`）；④ **区分力**：若 Dev 漏了这一步，`TITLES.len() == 99` 而实际 107 → 该测试**必红**；⑤ 断言 `TITLES` 实际长度 **== 107**（逐字），且**没有重复 code**（0 个重复）

### TC-015-082 `all_codes()` 已加 `platform_notify::error::ALL_CODES`，8 个新码全覆盖
- **AC**: AC-S-174 步骤 1c 续（`all_codes()` 假绿防线）｜ **类型**: 契约/安全 ｜ **优先级**: P0 ｜ **自动化**: 静态扫描 + 集成 ｜ **方法**: 因果
- **前置**: 同 TC-015-081
- **步骤**: 1) 读 `crates/platform-server/tests/i18n_titles.rs:59` 附近 `all_codes()` 的函数体 2) 确认其中**有** `for code in platform_notify::error::ALL_CODES { … }` 3) 检查 `titles_cover_every_code` 的通过状态 4) 数 8 个新码是否都在被检查的集合里
- **预期**: ① `all_codes()` 函数体**包含** `platform_notify::error::ALL_CODES`（逐字）；② 8 个新码**全部**出现在 `all_codes()` 的输出里：`NOTIFY_TEMPLATE_NOT_FOUND` / `NOTIFY_TEMPLATE_DUPLICATED` / `NOTIFY_TEMPLATE_MISSING_VARIABLE` / `NOTIFY_TEMPLATE_INVALID_PLACEHOLDER` / `NOTIFY_CHANNEL_NOT_CONFIGURED` / `NOTIFY_CATEGORY_MANDATORY` / `NOTIFY_NOT_DEAD_LETTER` / `NOTIFY_SENSITIVE_PAYLOAD`（**逐字 8 条，双向核对**）；③ **关键（防假绿）**：漏加这一行时 `titles_cover_every_code` 会**通过**（因为它根本没看到这 8 个码），但表里缺条目也不报 —— 所以本条**必须**断言 ① 的源码事实 + ② 的集合内容，**不能**只看测试是绿的；④ 若 ① 缺失，本条红，即使 TC-015-081 绿

### TC-015-083 两个词库的 `error.*` 各 107 条，8 个新码 zh-CN/en-US 都有
- **AC**: AC-S-174 步骤 1c（`every_error_code_is_registered_in_every_locale`，`:382`）｜ **类型**: 契约 ｜ **优先级**: P0 ｜ **自动化**: 集成 ｜ **方法**: 边界值
- **前置**: 两个 locale 词库文件存在（`zh-CN` / `en-US`）
- **步骤**: 1) 数 `zh-CN` 词库里 `error.*` 条目数 2) 数 `en-US` 词库里 `error.*` 条目数 3) 对 8 个新码逐个在**两个**词库里查 `error.<CODE>`
- **预期**: ① `zh-CN` 的 `error.*` **恰好 107** 条；② `en-US` **恰好 107** 条；③ 8 个新码 × 2 个 locale = **16 个键全部存在**且值非空（**逐条断言，不是「数量对了就行」**）；④ 负向断言：两库各**不是 99**（未加）；⑤ 中文词条不是英文拷贝（`zh-CN` 的值含 CJK 字符，用 `chars().any(|c| c as u32 > 0x4E00)` 断言）—— 防止「英文文案占位」

### TC-015-084 `notify.*` 4 个事件码**不得**出现在 `TITLES` 里
- **AC**: AC-S-174 步骤 1c（`org.*` vs `notify.*` 口径）｜ **类型**: 契约 ｜ **优先级**: P0 ｜ **自动化**: 静态扫描 + 集成 ｜ **方法**: 边界值
- **前置**: `crates/platform-core/src/titles.rs` 已交付
- **步骤**: 1) 在 `TITLES` 里查 `notify.template.created` / `notify.template.updated` / `notify.template.deleted` / `notify.dead_letter.resent` 2) **阳性对照**：在 `TITLES` 里查 `org.` 前缀的事件码（应在）
- **预期**: ① 4 个 `notify.*` 码在 `TITLES` 里**命中数 0**（**逐字 0**）；② 阳性对照：`org.*` 前缀在 `TITLES` 里命中 **> 0**（证明扫的是真的 `TITLES` 表，不是空集合）；③ 依据（D223）：审计事件码不是 HTTP 错误码，不进 `TITLES`；④ `TITLES` 总行数**仍是 107**（若把 4 个塞进去会变 111 → 与 TC-015-081 的钉死断言联动报红）；⑤ 反面区分：`org.*` **在** `TITLES` 里是因为它当「通知标题文案来源」被 `title_of_code()` 查（`crates/platform-org/src/notify.rs:144`），`notify.*` 没人查 → 所以不进

## 步骤 2：启动对账 fail-fast

### TC-015-085 只改 manifest 不加种子 INSERT → 服务起不来，日志逐条列出缺失权限点
- **AC**: AC-S-174 步骤 2 ｜ **类型**: 安全/可靠性 ｜ **优先级**: P0 ｜ **自动化**: 集成（进程启动）｜ **方法**: 因果
- **前置**: `crates/platform-notify/manifest.json` 有 3 个权限点；**权限点种子迁移未执行**（或从种子表里删掉这 3 条）
- **步骤**: 1) 起服务（`cargo run -p platform-server` 或测试用 `spawn`）2) 读退出码与日志
- **预期**: ① 进程**退出码非 0**（**不是**起成功了再降级）；② 日志里**逐条**列出 3 个缺失的权限点编码：`notify:template:read` / `notify:template:write` / `notify:delivery:read`（**3 条都出现**，不是只报第一个）；③ 每条日志都带来源 manifest 路径 `crates/platform-notify/manifest.json`（**逐字**）；④ **负向断言**：进程**没有**在 8080（或测试端口）上 accept 连接（用 connect 探测失败）—— 证明是真 fail-fast 而不是「日志报错但照跑」

### TC-015-086 启动对账的阳性对照：种子补齐后正常启动
- **AC**: AC-S-174 步骤 2（对照）｜ **类型**: 可靠性 ｜ **优先级**: P1 ｜ **自动化**: 集成（进程启动）｜ **方法**: 因果
- **前置**: 执行权限点种子迁移，3 条都进库
- **步骤**: 1) 起服务 2) 探测端口
- **预期**: ① 进程**不退出**（3 秒内仍存活）；② 端口可连；③ 日志**不含**「缺失权限点」字样；④ 与 TC-015-085 配对：同样代码、只差种子 → 一个失败一个成功，证明对账逻辑真的在查库而不是恒失败

## 步骤 3：C-5 阳性对照（删掉 require_permission 调用点 → 必须红）

### TC-015-087 把某权限点的 `require_permission` 调用点改成非字面量 → C-5 失败
- **AC**: AC-S-174 步骤 3 ｜ **类型**: 契约 ｜ **优先级**: P0 ｜ **自动化**: 契约（阳性对照，临时补丁）｜ **方法**: 变异检测
- **前置**: 在临时分支（**不提交**）把 `notify:delivery:read` 的 `require_permission("notify:delivery:read")` 改成 `require_permission(&dynamic_str)`（非字面量）
- **步骤**: 1) 跑 `cargo test -p platform-notify --test contract_c5` 2) 观察结果 3) 还原补丁
- **预期**: ① 测试**失败**（`1 failed` 或以上）；② 失败信息**点名** `notify:delivery:read`（已登记但无鉴权调用点）；③ 还原后再跑 → 通过（证明变异是唯一变量）；④ 本条的价值：证明 TC-015-080 的绿**不是**因为 C-5 什么都没检查（若 C-5 是空实现，本条也会绿 → 这才是假绿）

## 步骤 4：C-5 命名禁区（第二个鉴权入口）

### TC-015-088 在 `crates/platform-notify/src/` 放一个 `authorize` 函数 → C-5 失败
- **AC**: AC-S-174 步骤 4 ｜ **类型**: 安全/契约 ｜ **优先级**: P0 ｜ **自动化**: 契约（阳性对照，临时补丁）｜ **方法**: 变异检测
- **前置**: 在临时分支（**不提交**）往 `crates/platform-notify/src/` 加 `fn authorize(...) {}`（D216 的 8 个别名之一，`crates/platform-contracts/src/authz_scan.rs:48` 的禁区表）
- **步骤**: 1) 跑 `cargo test -p platform-notify --test contract_c5` 2) 观察结果 3) 还原
- **预期**: ① 测试**失败**；② 失败信息点名 `authorize` 与所在文件路径；③ 还原后通过；④ 本条证明命名禁区有效 —— design §10 提醒本域极易踩 `authorize`/`check_permission`/`can`/`allow`（「是否允许给该用户发」听着像鉴权，实际不是，应用 `is_recipient_eligible`）

### TC-015-089 本 crate 源码里不含 C-5 的 8 个禁区函数名（交付体检）
- **AC**: AC-S-174 步骤 4（正向）｜ **类型**: 安全/可维护性 ｜ **优先级**: P1 ｜ **自动化**: 静态扫描 ｜ **方法**: 边界值
- **前置**: 本轮交付的 `crates/platform-notify/src/` 已就位
- **步骤**: 扫 `crates/platform-notify/src/` 中作为**函数名**出现的 `authorize` / `check_permission` / `can` / `allow` 等 8 个别名
- **预期**: ① 命中数 **0**；② **阳性对照**：同一扫描在 `crates/platform-contracts/src/authz_scan.rs:48` 的禁区表上命中 **> 0**（证明扫描器能识别这些名字，不是正则写错导致恒 0）；③ 若在注释里出现（如「不能叫 authorize」）允许，但**作为 `fn` 名不得出现** —— 断言用 `fn\s+(authorize|check_permission|can|allow)\b` 而非裸词匹配

## 步骤 5：审计埋点（4 个事件码进 platform_audit_log）

### TC-015-090 走一遍建/改/删模板 + 重投死信 → 恰好 4 个事件码进审计表
- **AC**: AC-S-174 步骤 5 ｜ **类型**: 功能/安全 ｜ **优先级**: P0 ｜ **自动化**: 集成（真实库）｜ **方法**: 等价类
- **前置**: `tenant-a` 管理员持 `notify:template:write` + `notify:delivery:read`；有 1 条 `dead_letter` 记录
- **步骤**: 1) `POST` 建模板 2) `PATCH` 改模板 3) `DELETE` 删模板 4) `POST /deliveries/{D_dead}/resend` 5) 查 `platform_audit_log`（`WHERE tenant_id='tenant-a'`）
- **预期**: ① 审计表**恰好新增 4 行**；② 4 行的 `action` **双向相等**于 `{notify.template.created, notify.template.updated, notify.template.deleted, notify.dead_letter.resent}`（**逐字 4 条，不多不少**）；③ 每个码**恰好出现 1 次**（不是某个码出现 2 次）；④ 没有其它 `notify.*` 码混进来（如 `notify.delivery.delivered` —— 投递成功不是审计事件）；⑤ 顺序：`created` → `updated` → `deleted` → `dead_letter.resent`（按 `created_at` 升序）

### TC-015-091 每条审计记录含契约 C12 的 9 个固定字段且 object_type 恰好两种
- **AC**: AC-S-174 步骤 5 字段集 ｜ **类型**: 安全 ｜ **优先级**: P0 ｜ **自动化**: 集成（真实库）｜ **方法**: 边界值
- **前置**: TC-015-090 的 4 行已产生
- **步骤**: 逐行读 4 行的字段集合与 `object_type` / `object_id`
- **预期**: ① **每一行**都含契约 C12 的 9 个字段且**非空**（`created_at` 除外可空校验按 C12）：`actor_id` / `action` / `object_type` / `object_id` / `ip` / `user_agent` / `created_at` / `prev_hash` / `hash`；② `actor_id` **逐字等于** `u-perm-notify@example.com`（不是空、不是 `system`）；③ `object_type` **恰好**是 `platform_notify_template` 或 `platform_notify_delivery`（表名语义，**双向断言**：4 行里只出现这两种字符串）；④ 前 3 行（模板的 created/updated/deleted）`object_type == "platform_notify_template"`；⑤ 第 4 行（`dead_letter.resent`）`object_type == "platform_notify_delivery"` 且 `object_id == D_dead`；⑥ `ip` 是合法 IP 字面量、`user_agent` 非空

### TC-015-092 审计哈希链可验证：`ChainVerifier::verify(tenant-a)` → ok
- **AC**: AC-S-174 步骤 5 链完整性 ｜ **类型**: 安全 ｜ **优先级**: P0 ｜ **自动化**: 集成（真实库）｜ **方法**: 因果
- **前置**: TC-015-090 的 4 行已产生
- **步骤**: 1) 调 `ChainVerifier::verify("tenant-a")` 2) 读 `ok` 与 `prev_hash` 链
- **预期**: ① `verify(...).ok == true`；② 4 行的 `prev_hash` → `hash` 链**连续**（第 N 行的 `prev_hash` **逐字节等于**第 N-1 行的 `hash`）；③ 第一行的 `prev_hash` 是链首值（全 0 或上一批尾部，按 C12 口径）；④ `hash` 值**互不相同**（4 个不同 hash）；⑤ 若篡改任一行内容再 verify → `ok == false`（**阳性对照**：证明 verifier 真在算，不是恒 true）

### TC-015-093 审计不记录到 `tenant-b`（租户隔离）
- **AC**: AC-S-174 步骤 5（隔离补强）｜ **类型**: 安全 ｜ **优先级**: P1 ｜ **自动化**: 集成（真实库）｜ **方法**: 边界值
- **前置**: TC-015-090 的操作全在 `tenant-a`
- **步骤**: 1) 以 `tenant-b` 查 `platform_audit_log` 2) 以 `base_app` + `SET app.tenant_id='tenant-a'` 查
- **预期**: ① `tenant-b` 视图下这 4 行的 `action` **命中 0**；② `tenant-a` 视图下**恰好 4 行**；③ `ChainVerifier::verify("tenant-b")` 对 `tenant-a` 的事件**不纳入**（各租户链独立）；④ 库层：审计表的 RLS 对 `tenant_id` 生效

---

## 〔AC 修正〕汇总（详见 index.md）

| 位置 | AC 原文缺口 | 本用例集给的修正后判据 |
|---|---|---|
| AC-S-169 步骤 3 | 没说 `attempt_count` 是 0 还是 1 | 实现必须钉死一个值，测试断言该具体值（TC-015-043 ⑤） |
| AC-S-173 步骤 4 | 只说「camelCase」，未说命中路径 | 补 `normalize_sensitive_key` 的 `clientSecret`→`client_secret`、`apiKey`→`api_key` 两条**逐个**断言（TC-015-053 ③⑤） |
| AC-S-172 步骤 5 | 说「恰好 1 个 200、1 个 409」但没给并发构造法 | 用 `Barrier` + 直接双 SQL 的 `rows_affected` 集合断言 `{1,0}`（TC-015-076） |
| AC-S-170 步骤 5 | 「10 分钟前」是相对阈值，未给未超阈值的对照 | 补 5 秒未超阈值不回收的对照（TC-015-062） |
| AC-S-166 步骤 7 | 「含超时（10 秒）」未给耗时区间 | 补 `10s ≤ elapsed ≤ 12s` 与「未进 AUTH」（TC-015-007） |
| AC-S-166 步骤 2 | 未要求 `delivered` 不得为 `false` 形态 | 明确禁止 `Ok(DeliveryResult{delivered:false})`（TC-015-002 ②） |
| AC-S-167 步骤 11 | 未说 eval 副作用如何处理 | 用 `catch_unwind` 断言不 panic 且返回 `Err(INVALID_PLACEHOLDER)`（TC-015-033 ⑤） |