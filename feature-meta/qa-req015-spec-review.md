# REQ-015 规格挑刺报告

- **基线**: `develop` @ `a2b7e52` ｜ **规格**: `acceptance.md` rev 2 / `decisions.md` D291~D307
- **执行**: BA 自查 + QA 用例棒（`TC-015-001~093`）
- **日期**: 2026-11-21

> ⚠️ **独立性声明（如实记）**：本报告由 **BA 自己**完成挑刺。
> 原本派了两轮 QA 子 Agent 做独立挑刺，**两轮都卡在写文件上**（30+ 分钟仅产出 5 行骨架，
> 无内容增量），BA 只能停掉并接管。
> → 因此本报告的**独立性弱于前三轮的第三方复核**（REQ-022 / REQ-016 的挑刺都是独立 Agent 做的）。
> 用例棒的 QA Agent 贡献了本报告的 **S-05 ~ S-11**（它在 `test-cases/index.md` 里
> 自行列出了 7 处〔AC 修正〕），这部分**有第三方成分**；
> **S-01 ~ S-04 完全由 BA 自查得出**，是本报告独立性最弱的部分，建议独立复核时重点重查。

---

## 总览

| 编号 | 严重度 | 主题 | 影响的 AC |
|---|---|---|---|
| S-01 | **高** | SMTP **头注入防御**在 design 里写了，但**没有任何 AC 验它** | AC-S-166 |
| S-02 | **高** | SMTP **base64 正文每 76 字符换行**同上：写了没验 | AC-S-166 |
| S-03 | **高** | **「STARTTLS 后重发 EHLO」没有 AC 判据** —— 而这是 SMTP 实现最易漏的一步 | AC-S-166 |
| S-04 | **高** | 3 张表声明了 RLS，但**只有 1 张**（delivery）有 AC 验 RLS | AC-S-173 |
| S-05 | **中** | AC-S-169 步骤 3 未钉死 `attempt_count` 是 0 还是 1 | AC-S-169 |
| S-06 | **中** | AC-S-170 步骤 5「10 分钟」只有超阈值一侧，缺未超阈值对照 | AC-S-170 |
| S-07 | **中** | AC-S-172 步骤 5 要并发，但**没给构造法** | AC-S-172 |
| S-08 | **中** | AC-S-166 步骤 7 的「10 秒超时」未给**可断言区间** | AC-S-166 |
| S-09 | **中** | AC-S-167 步骤 11 的 SSTI 载荷未说「若真被 eval 了怎么判」 | AC-S-167 |
| S-10 | **低** | AC-S-166 步骤 2 未封死 `Ok(DeliveryResult{delivered:false})` 形态 | AC-S-166 |
| S-11 | **低** | AC-S-174 步骤 1c 的 `all_codes()` 判据有「漏加时恰好是绿的」风险 | AC-S-174 |

**合计 11 条**：高 4 / 中 5 / 低 2。**阻塞 0 条**（没有「无法实现」级的缺陷）。

---

## 高

### S-01: SMTP **头注入防御**在 design 里写了，但没有任何 AC 验它

- **问题**: `design-summary.md` §4.6 写着：
  > ⚠️ **头注入防御**：`To` / `From` / `Subject` 里的 `\r` / `\n` **必须剥除或拒绝**。
  > 否则 `Subject: x\r\nBcc: attacker@evil` 会注入额外收件人 —— **这是一条安全判据**。

  但 `acceptance.md` 的 AC-S-166 里，`头注入` / `\r\n` 各出现 **0 次**。
- **证据**:
  ```
  $ grep -c "头注入" BA/demands/REQ-015-notification/acceptance.md
  0
  $ grep -c "76" BA/demands/REQ-015-notification/acceptance.md
  0
  $ grep -c "EHLO" BA/demands/REQ-015-notification/acceptance.md
  0
  ```
  **阳性对照**（证明 grep 模式本身有效，不是模式写坏了）：
  ```
  $ grep -c "AUTH LOGIN" BA/demands/REQ-015-notification/acceptance.md
  1
  $ grep -c "provider_message_id" BA/demands/REQ-015-notification/acceptance.md
  2
  ```
  → 「0 命中」不是因为 grep 坏了，是因为 AC 里真的没写。
- **影响**: 若 Dev 不做头注入防御，**没有任何判据会红**。
  而这是**真实可利用的漏洞**：通知的 `Subject` 往往由模板变量渲染
  （`{{userName}}`），租户管理员能编辑模板、普通用户的显示名也会进 payload
  → 注入 `\r\nBcc: attacker@evil.com` 就能**把所有通知抄送一份给攻击者**。
  与 REQ-016 的 `javascript:` 是同一类：**规格里写了但没判据 = 等于没要求**。
- **建议**: AC-S-166 加**路径 7**：`payload = {userName: "x\r\nBcc: attacker@evil.example"}`，
  模板 `{{userName}}` 进 `Subject` → 断言假服务器收到的 `DATA` 里
  **`Bcc:` 出现 0 次**（阳性对照：`To:` 出现 1 次）。
  **必须用「假服务器实际收到的字节」做判据**，不能只断言「返回了错」。

### S-02: SMTP base64 正文**每 76 字符换行**同上：写了没验

- **问题**: `design-summary.md` §4.6 写着：
  > ⚠️ **base64 正文必须每 76 字符换行**（RFC 2045）；不换行会被严格 MTA 拒。

  AC 里 `76` 出现 **0 次**（见 S-01 的实测）。
- **影响**: `Content-Transfer-Encoding: base64` 而正文不按 76 字符折行 →
  违反 RFC 2045 §6.8，**部分 MTA 直接拒收**（表现为「偶发投递失败」，极难定位）。
- **建议**: 在 AC-S-166 的 SMTP 段加判据：假服务器收到的 `DATA` 正文里，
  **除最后一行外每行 base64 长度恰好 76**；且总行数 = `ceil(len(base64)/76)`。
  ⚠️ 要**逐行算长度**，不能只断言「含换行」。

### S-03: **「STARTTLS 后重发 EHLO」没有 AC 判据** —— 而 D305 自己说这条最关键

- **问题**: `design-summary.md` §4.6 写着：
  ```
  EHLO <host>  → 250-…（**TLS 之后必须重发**：能力集变了，TLS 前看不到 AUTH）
  ```
  `decisions.md` 的 **D305** 也写着：
  > 「STARTTLS 后重发 EHLO」那条尤其关键 —— 它是 SMTP 实现里最常被漏掉的一步

  但 AC 里 `EHLO` 出现 **0 次**。
- **影响**: 这是 SMTP 客户端**最经典的功能性 bug**：不少服务器的 `AUTH` 能力
  **只在 TLS 之后**才宣告（为了阻止明文传密码）。客户端若不在 TLS 后重发 `EHLO`，
  它看到的能力集里没有 `AUTH` → **永远认证失败**，或（更糟）**静默降级到明文认证**。
  没有判据 → 写错了也是绿的。
- **建议**: 假服务器 `auth_only_after_tls: true`（**只在 TLS 之后的 `EHLO`** 响应里带
  `250-AUTH LOGIN`）→ 断言认证成功。
  阳性对照：让假服务器记 `EHLO` 次数 → 断言 **恰好 2 次**（TLS 前 1 + TLS 后 1）。
  ⚠️ 只断言「成功」不够（客户端可能压根没做 TLS）；**必须同时断言 EHLO 次数 = 2**。

### S-04: 3 张表声明了 RLS，但只有 1 张有 AC 验 RLS

- **问题**: `design-summary.md` §2.4 声明 3 张表都进 `tenantScoped.tables`：
  `platform_notify_template` / `platform_notify_preference` / `platform_notify_delivery`。
  但 AC 里 `RLS` 只出现在 **AC-S-173 步骤 6**（**只验 delivery 一张表**）。
  `grep -n "tenant-policies\|policy" acceptance.md` → **0 命中**。
- **影响**: `template` 与 `preference` 两张表的 RLS **完全没有判据**。
  若 Dev 在 manifest 里漏写某张表的 `tenantScoped` 声明，
  `cargo xtask tenant-policies --check` 会绿（没声明就不生成也不校验），
  而那张表**完全无隔离** → **跨租户读别人的通知模板 / 订阅偏好**。
  REQ-016 的 `AC-S-157` 就是专门验这个的。
- **建议**: AC-S-173 加一步：① 断言 `manifest.json` 的 `tenantScoped.tables`
  **恰好**是那 3 张表（双向相等）；② `cargo xtask tenant-policies --check` **退出码 0**；
  ③ 对 `template` 与 `preference` **各做一次**直连 RLS 验证（同步骤 6 手法，换表）。

---

## 中

### S-05: AC-S-169 步骤 3 未钉死 `attempt_count` 是 0 还是 1

- **问题**: AC-S-169 步骤 3 预期写「该条进死信，`status == "dead_letter"`，
  `last_error` 指明用户已退订」—— **没说 `attempt_count`**。
- **影响**: 两种实现都能过：「退订检查在出队前」→ `attempt_count = 0`；
  「出队后投递前」→ `attempt_count = 1`。**两种都合理**，但若不钉死，
  「退订后还重试 5 次才进死信」这种**明显错误**的实现也能过（它的 `last_error` 也是退订相关）
  → 判据留空间 = 给「无效重试」开门。
- **建议**: 钉死 **`attempt_count == 0`**：「用户已退订」是**确定性终态**，
  重试 5 次不会改变结论 → 重试纯属浪费且拖慢 worker。**写进 AC，不留给 Dev 选。**

### S-06: AC-S-170 步骤 5「10 分钟前」只有超阈值一侧，缺未超阈值对照

- **问题**: 只验「`delivering` 且 `updated_at` 是 10 分钟前 → 应被回收」。
  没有对照：`delivering` 但 `updated_at` 是 5 秒前 → **不应**被回收。
- **影响**: 「只要看到 `delivering` 就抢」的实现也能过 → 会在正常投递进行中把行抢走
  → 同一通知被两个 worker 同时投递（**重复投递**，正是 D296 要避免的）。
- **建议**: 补阴性对照：`delivering` + `updated_at = now - 5s` → `tick()` 后该行**仍不被取**。
  且把阈值钉死：**新增配置项 `notify.deliveringTimeoutSeconds`（默认 300）** → 加进 D304。

### S-07: AC-S-172 步骤 5 要并发，但没给构造法

- **问题**: 写「**并发**：对 `D_dead` 同时发 2 个 `resend`」，没说在测试里怎么构造。
- **影响**: 若 Dev 写成串行（发一次 → 再发一次），第一个已改成 `pending`，
  第二个返回 409 → **判据绿的，但根本没测并发**。典型「看起来测了并发，其实测了串行」。
- **建议**: 给构造法：`tokio::sync::Barrier::new(2)` 让两个 task 同一时刻发；
  或更可靠地**直接对 SQL 层双语句并发**（两个 `TenantTx` 各 `execute` 同一条
  `UPDATE ... WHERE status='dead_letter'`），断言 `rows_affected` 集合 **恰好 `{1, 0}`**
  （⚠️ **不是** `{1}`，也不是「两个都 1」）。

### S-08: AC-S-166 步骤 7 的「10 秒超时」未给可断言区间

- **问题**: 预期「`error` 含超时（10 秒）」。没说测什么。
- **影响**: 「含超时」可能只是「错误文案里出现 `timeout`」——
  一个**立刻失败（没真等）**的实现也能造出含 `timeout` 的文案。
- **建议**: 断言**实际耗时区间** `10s <= elapsed <= 12s`（下界证明真等了，
  上界防实现把 timeout 写成 60s 而判据仍绿）+ `call_count("AUTH") == 0`。

### S-09: AC-S-167 步骤 11 的 SSTI 载荷未说「若真被 eval 了怎么判」

- **问题**: 给了 4 个 SSTI 载荷，预期「全部 `INVALID_PLACEHOLDER`」。
  没说「如果实现真的 eval 了会怎样」。
- **影响**: 若载荷导致 **panic**（索引越界等），测试会以 panic 结束，
  看起来是「测试崩了」而不是「发现漏洞」。
- **建议**: 用 `std::panic::catch_unwind` 包住渲染调用，断言
  ① 不 panic；② 返回 `Err` 且错误码正确。两条都要，缺一不可。

---

## 低

### S-10: AC-S-166 步骤 2 未封死 `Ok(DeliveryResult{delivered:false})` 形态

- **问题**: 步骤 3/4 说未配置通道「`send()` **必须返回 `Err`**」，
  但步骤 2 只写「必须返回 `Err`，`delivered` **不得**为 `true`」
  → 这句话允许 `Ok(DeliveryResult{delivered:false})`。
- **影响**: `trait NotificationSender::send` 返回 `Result<DeliveryResult>`，
  所以「未配置」既可以是 `Err` 也可以是 `Ok(delivered:false, error:Some(…))`。
  后者会让 worker 当作「投递完成但失败」→ 走**重试**分支
  → **对未配置的通道重试 5 次**才进死信。与 D292 的铁律冲突。
- **建议**: 改成「**必须返回 `Err`**；**不得**返回 `Ok`（含 `Ok(delivered:false)`）」。

### S-11: AC-S-174 步骤 1c 的 `all_codes()` 判据有「漏加时恰好是绿的」风险

- **问题**: 步骤 1c 说「`all_codes()` 必须加 `platform_notify::error::ALL_CODES`，
  否则**会假绿**」。但**判据本身**写的是「步骤 1c 通过」——
  而漏加时它**恰好就是绿的**（这正是本条自己指出的事实）。
- **影响**: 判据与它要防的东西**同向**：文字说了「漏加会假绿」，
  但验收方式仍是「看测试绿不绿」→ **同一个假绿**。
- **建议**: 判据改成**读源码断言**：
  `grep -c "platform_notify::error::ALL_CODES" crates/platform-server/tests/i18n_titles.rs` **≥ 1**
  （阳性对照：`grep -c "platform_app::error::ALL_CODES"` **= 1**）。
  即「断言那行代码存在」而不是「断言测试绿」。

---

## BA 已自查通过的部分（记录，防止后人重复劳动）

以下事实由 BA 实测确认，**不是问题**：

| 项 | 实测值 | 依据 |
|---|---|---|
| `TITLES` 条目数 | **99**（0 重复 code） | python 正则解析 `titles.rs:45` |
| `permissions.json` `total` | **35** = 6+3+3+4+6+9+4 | python 读 json |
| `capabilities/` 文件数 | **14** = 7 固定 + 7 sdk | `find capabilities -type f` |
| 迁移最后编号 | `20261121000300_app_config_seed` | `ls migrations/*.up.sql` |
| `manifest.json` 数量 | **6**（新增后 7） | `ls crates/*/manifest.json` |
| AC 引用的 9 处 `file:line` | **全部命中目标符号** | python 逐区间验证 |
| `apiKey` / `clientSecret` 归一化 | **会命中**（D172 已修） | `audit.rs:452-453` + 23 passed |
| `SKIP LOCKED` 在 `TenantQuery` 下可行 | **可行**（只做占位符改写，不解析语句形态） | `query.rs:251-287` |
| `FOR UPDATE` 先例 | **3 处**（`SKIP LOCKED` 为 **0**，配阳性对照） | `grep -rn` |
| `scan_sensitive_keys` 可复用 | `platform-core/src/extension/audit.rs:430` | 已读函数体 |
| i18n `error.*` 条数 | **99**（`zh-CN` / `en-US` 各） | python 读 json |

---

## 裁决（BA 全部接受，见 D308~D318）

11 条**全部接受**，全部回填进 `acceptance.md` rev 3。逐条对应见 `decisions.md` 的
**D308 ~ D318**。

其中 **S-06** 新增第 5 个配置项 `notify.deliveringTimeoutSeconds`（默认 300）
→ **D304 的配置项从 4 个变 5 个**，`D297` 的退避参数表同步更新。
