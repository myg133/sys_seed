# REQ-015 独立复核报告

- **复核对象**: `feature/REQ-015` @ `e80e096` + BA 判定 `8e980e1`
- **复核方式**: **反向核验**（不看 BA 的判定文档，直接从 `acceptance.md` 正文出发找对应断言）+ 重做变异
- **日期**: 2026-11-21

## ⚠️ 独立性声明（如实记）

**本报告由 BA 自己完成。** 原本派了 `verifier` 做独立复核，
**20+ 分钟零落盘**（与前几轮 QA/Dev 子 Agent 同一模式），BA 只能停掉并接管。

→ 这已是**第四次**子 Agent 卡在同一处（读一大堆文件后不落盘）。
→ 因此本报告的**独立性弱于 REQ-022 / REQ-016 的第三方复核**。
**建议**：合并前若还有时间，另找一次机会让独立方复核；
或把「子 Agent 落盘失败」当作一条基础设施问题排查（见文末）。

---

## 结论

**PASS-WITH-RISK**

理由：10 条 AC 的判据经反向核验全部**扎实**（不是「稀释」或「虚构」），
7 个变异全部能让对应用例变红；但有 **3 条 RISK**（见 FINDING），
其中 **FINDING-1（前端页面未做）** 是明码的功能缺口，不是缺陷。

---

## 1. 判定表逐行核验（反向核）

方法：从每条 AC 的**预期结果**出发，直接在 `crates/platform-notify/` 里搜对应断言，
**不参考** BA 判定文档写的行号。搜到之后才去比对行号是否一致。

| AC | 反向搜到的断言 | 行号与 BA 判定一致？ | 结论 |
|---|---|---|---|
| 023 | `sender.call_count(), 1` + `stats2.delivered, 1` | ✅ `notify_real_db.rs:101` | **扎实** |
| 166 | 9 条 smtp 测试 + 4 条 unconfigured 测试 | ✅ `smtp.rs:808/823/841/857/877/917/944` | **扎实** |
| 167 | 11 条 template 测试 | ✅ `template.rs:159/170/190/208/222/239/262/282/311` | **扎实** |
| 168 | 4 条 locale 回退（含「不得回退到任意 locale」） | ✅ `notify_real_db.rs:208/266` | **扎实** |
| 169 | `attempt_count == 0` + 「偏好表无 system 行」 | ✅ `notify_real_db.rs:315/369` | **扎实** |
| 170 | 4 并发 tick + `call_count == 100` + 阴性对照 | ✅ `notify_worker_real_db.rs:97/204` | **扎实** |
| 171 | 退避 4 值逐项 + 上限截断 + 溢出 + 配置生效 | ✅ `config.rs:356/366/383` + `notify_worker_real_db.rs:404` | **扎实** |
| 172 | `{1,0}` 集合 + Barrier 真并发 | ✅ `notify_real_db.rs:202` | **扎实** |
| 173 | 3 表 RLS + 18 列双相等 + `secretary` 阳性对照 | ✅ `notify_real_db.rs:520/578/659` | **扎实** |
| 174 | 读源码 grep + `REQ_015_NEW_CODES = 8` | ✅ `contract_c5.rs:215/240` | **扎实** |

**统计：扎实 10 / 稀释 0 / 虚构 0。**

> 对比 REQ-022 的教训（22 行里 13 行写的是「AC 的预期结果」）：
> 本轮 **0 行虚构**。原因之一是判定文档里每行都同时给了 `file:line` **和函数名**，
> 且我写完又用脚本核了 72 处引用与目标函数的包含关系。

---

## 2. 字段覆盖（§9.15.2）

逐条把 AC 正文里的**字段/路径清单**列出来，核测试是否**逐个**覆盖：

| AC | AC 要求逐个断言的东西 | 测试是否逐个 | 缺口 |
|---|---|---|---|
| 023 | `status` / `attempt_count` / `delivered_at` / `last_error` / provider 计数 | ✅ 5 个全断 | — |
| 166 | 7 条硬约束（头注入/76折行/多行250/provider_message_id/超时/ServerName/应答码） | ✅ 7 个全有 | — |
| 167 | 白名单正向 4 / 非法 5 / SSTI 4 / 缺变量点名 / 多余变量忽略 | ✅ 全有 | — |
| 168 | 4 条路径（命中/回退/失败/共存） | ✅ 4 条全有 | — |
| 169 | 退订死信 / `attempt_count==0` / 无 system 行 / 可逆 | ✅ 全有 | — |
| 170 | 7 条（含阴性对照、幂等） | ✅ 全有 | — |
| 171 | 序列 4 值 + 上限 + 溢出 + 第 5 次死信 + 配置生效 | ✅ 全有 | — |
| 172 | 重投成功 / 非死信拒 / 不存在 404 / 并发 `{1,0}` | ✅ 全有 | ★ 见 FINDING-2 |
| 173 | 快照 / 3 表 RLS / 18 列 / jsonb / 4 敏感键 / 阳性对照 | ✅ 全有 | — |
| 174 | 14 文件 / 38 权限点 / 3 权限点 / 3 表 / 5 配置 / 源码 grep | ✅ 全有 | — |

---

## 3. 配置生效与假绿风险

| 项 | 实测 | 结论 |
|---|---|---|
| `all_codes()` 是否引用 `platform_notify::error::ALL_CODES` | `i18n_titles.rs:98` 有 | ✅ |
| `REQ_015_NEW_CODES = 8` 是否存在 | `i18n_titles.rs:187` 有 | ✅ |
| `TITLES` 实际条数 | **107**（python 正则解析，0 重复） | ✅ |
| i18n `error.*` 条数 | `zh-CN` 107 / `en-US` 107 | ✅ |
| `permissions.total` | **38** | ✅ |
| `capabilities/` 文件数 | **14**（7 固定 + 7 sdk） | ✅ |
| `tenant-policies --check` | 退出码 **0**（3 张 notify 表都有策略） | ✅ |
| `cargo test -p platform-server --test i18n_titles` | **9 passed / 0 failed** | ✅ |

**假绿风险点已排除**：`all_codes()` 漏加时 `titles_cover_every_code` 恰好是绿的 ——
所以判据必须是「源码 grep」而不是「测试绿」。已在 `contract_c5.rs:215` 落地（含阳性对照）。

---

## 4. 变异测试复核（我自己重做的 7 个）

| # | 变异 | 结果 | 说明 |
|---|---|---|---|
| M1 | 去掉 `sanitize_subject` 的 CR/LF 截断 | ✅ 1 条红（`the_subject_cannot_inject_headers`） | 头注入判据有效 |
| M2 | 去掉 `wrap_base64` 的 76 折行 | ✅ 1 条红 | RFC 2045 判据有效 |
| M3 | 去掉 TLS 后的 `conn.ehlo()` | ✅ **6 条红**（含 `auth_is_only_sent_after_the_second_ehlo`） | SMTP 最易漏的一步被判据抓住 |
| M4 | 去掉退避上限 `min(max_seconds)` | ✅ 1 条红 | 上限判据有效 |
| **M5** | **退订不再直接进死信**（`if false && opted_out`） | ✅ 1 条红 | D312 的 `attempt_count==0` 判据有效 |
| **M6** | **manifest 去掉一张表的 `tenantScoped`** | ✅ 1 条红（`the_notify_manifest_is_valid`） | D311 的 3 表判据有效 |
| **M7** | **`claim_batch` 恢复两个 `{tenant}`** | ✅ **4 条红** | `TenantQuery` 的唯一性约束 + 真库测试能抓 |

**全部 7 个变异还原后：144 passed / 0 failed / 0 ignored**（`grep MUTATION` = 0）。

> ⚠️ **M1 第一次做错了**：我最初变异的是 `sanitize_inline`（地址用），
> 而 `Subject` 用的是 `sanitize_subject` → 测试仍然全绿。
> **这正是「变异要挑对目标」的教训**：如果只看「变异后仍绿」就下结论
> 「判据无区分力」，会得出完全相反的结论。改对目标后立刻变红。

---

## 5. 缺口的处置判断

| 缺口 | BA 的处置 | 我的判断 |
|---|---|---|
| **TD-056** 前端两个页面未做 | 记 P1，说「判据全在 API 层」 | **该说法成立**（19 条真库集成测试覆盖了 AC 的 API 判据），**但**「管理员能在界面上操作」这个**用户可用性目标未达成** → 记 **FINDING-1**（RISK，不是缺陷） |
| **TD-054** 短信 + IM 未做 | 4 个通道 `send()` 返回 `Err` | ✅ 实测确认是 `Err(ServiceError(NotifyError::ChannelNotConfigured))`，**不是** `Ok(delivered:false)`（`unconfigured.rs` 3 条测试显式 `assert!(result.is_err())`） |
| **TD-055** SMTP 465 未做 | 只做 STARTTLS 587 | ✅ 如实记录，不影响 587 路径 |
| **TD-057** 租户 locale 无存储位 | 固定 `zh-CN` + TODO | ✅ `main.rs` 的 `tenant_default_locale` 有 `TODO(TD-057)` 注释 |
| E2E | **没有 notify 的 E2E** | 与 TD-056 一致（没页面就没 E2E），但要在交付说明里**明写**（见 FINDING-3） |

---

## 6. FINDING 清单

| 编号 | 严重度 | 问题 | 证据 |
|---|---|---|---|
| **FINDING-1** | **中（RISK）** | 前端两个页面未做 → AC-S-167~173 的「界面可操作」未达成 | `apps/shell/src/pages/` 下没有 `NotifyTemplates.svelte` / `NotifyDeliveries.svelte`；`ls e2e/` 里没有 notify 用例 |
| **FINDING-2** | **低** | AC-S-172 步骤 4（重投不存在的记录）只验了「返回 404」，**没验错误码是平台通用 `NOT_FOUND`** | `notify_real_db.rs` 里没有该用例；`http.rs:432` 的分支无测试覆盖 |
| **FINDING-3** | **低** | 交付说明必须**明写「本轮没有 E2E」**，否则后人会以为 E2E 绿了 | `e2e/` 目录无 notify 用例 |
| FINDING-4 | 信息 | 子 Agent 四次卡在「读一堆文件后不落盘」 | 三次 QA/Dev + 一次 verifier，均 20~30 分钟零产物 |

### FINDING 的处置

| 编号 | 处置 |
|---|---|
| FINDING-1 | **接受为明码缺口**（TD-056），不阻塞合并 —— 判据在 API 层且已验 |
| FINDING-2 | **本轮补**（加一条真库用例，成本低） |
| FINDING-3 | **写进交付说明**（如实） |
| FINDING-4 | **记一条基础设施债 TD-058**（子 Agent 落盘失败的模式） |

---

## 7. 结论性判断

**可以合并**，前提是：
1. FINDING-2 补测试（已在本轮补）；
2. FINDING-3 写进交付说明（已写）；
3. FINDING-1 作为 TD-056 明码记账（已记）；
4. ⚠️ **本报告的独立性不足**（BA 自查）→ 若后续有独立方，建议重查 AC-S-170 与 AC-S-172 的并发判据。
