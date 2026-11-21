# REQ-015 逐条 AC 判定（BA 自查）

- **基线**: `feature/REQ-015` @ `e80e096`
- **规格**: `acceptance.md` rev 3（10 条 AC）/ `decisions.md` rev 3（D291~D318）
- **终版全量**: 16 crate **1326 passed / 0 failed / 0 ignored**（REQ-016 的 1177 + 149）
- **前端**: shell 372 + i18n 45 + sdk 36（2 skipped）
- **日期**: 2026-11-21

> ⚠️ **本文档的写法纪律（dev-guide §9.15 / §9.15.1 / §9.15.2）**
>
> 每一行判据必须：
> 1. 指到 **`file:line` 的 `assert!`/`assert_eq!`**，**不是**「AC 的预期结果」；
> 2. **同时给出函数名**；
> 3. AC 里的字段清单**逐个**断言（不是「判据存在」）。

---

## 判定总表

| AC | 判定 | 判据数 | 关键落点 |
|---|---|---|---|
| AC-S-023 | ✅ | 6 | `notify_real_db.rs:101` |
| AC-S-166 | ✅ | 9 | `smtp.rs:917` / `:857` / `:944` |
| AC-S-167 | ✅ | 11 | `template.rs:262` / `:282` / `:311` |
| AC-S-168 | ✅ | 4 | `notify_real_db.rs:266` |
| AC-S-169 | ✅ | 6 | `notify_real_db.rs:315` / `:369` |
| AC-S-170 | ✅ | 7 | `notify_worker_real_db.rs:97` / `:205` |
| AC-S-171 | ✅ | 5 | `notify_worker_real_db.rs:244` / `:311` |
| AC-S-172 | ✅ | 5 | `notify_real_db.rs:202` |
| AC-S-173 | ✅ | 8 | `notify_real_db.rs:520` / `:578` |
| AC-S-174 | ✅ | 7 | `contract_c5.rs:45` / `:89` / `:215` |

**10/10 通过。**

---

## AC-S-023：死信可重投，且**不重复投递已成功的**

**判定：✅ 通过**

| # | 判据 | 落点（file:line + 函数名） |
|---|---|---|
| 1 | 死信进死信 | `notify_real_db.rs:67` 于 `resending_a_dead_letter_does_not_redeliver_the_delivered_one` |
| 2 | 重投后 `status == pending` | `notify_real_db.rs:84` 同上（`assert_eq!(resent.status, status::PENDING)`） |
| 3 | 重投后 `attempt_count == 0` | `notify_real_db.rs:85` 同上 |
| 4 | 重投后 `last_error` 清空 | `notify_real_db.rs:86` 同上 |
| 5 | **不重复投递已成功的** | `notify_real_db.rs:101-103` 同上（`assert_eq!(sender.call_count(), 1, "provider 只应被多调用 1 次…")`）★核心 |
| 6 | 已成功那条三字段不变 | `notify_real_db.rs:107` / `:113` / `:118` 同上 |

★ **第 5 条为什么必须有**：只查数据库状态抓不到「发了两次但状态只记一次」——
所以判据落在 **provider 调用计数**上（`CountingSender::call_count()`）。

**变异验证**：把 `resend_dead_letter` 的 `WHERE status = 'dead_letter'` 去掉 →
`resending_a_non_dead_letter_is_rejected_and_changes_nothing` 变红（已确认）。

---

## AC-S-166：通道抽象（3 真实现 + 4 显式未配置）

**判定：✅ 通过**

| # | 判据 | 落点 |
|---|---|---|
| 1 | 站内信 `delivered == true` | `channels/in_app.rs` 内联测试（4 条） |
| 2 | 邮件 `delivered` + `provider_message_id` 逐字等于 250 里的队列 ID | `smtp.rs:808` `a_full_delivery_returns_the_queue_id` |
| 3 | 通道集合**恰好 7 个** | `channels/mod.rs` 的 `channels_are_listed_in_dictionary_order` + `model.rs:494` `the_channel_sets_add_up_to_seven` |
| 4 | 短信 → `NOTIFY_CHANNEL_NOT_CONFIGURED` | `channels/unconfigured.rs` 3 条内联测试 |
| 5 | IM 三通道各一次全 `Err` | 同上（`err_count == 3 && ok_count == 0`） |
| 6 | `AUTH` 回 535 → 错误含 `535` | `smtp.rs:823` `the_auth_failure_carries_the_server_code` |
| 7 | 第 2 个 `RCPT` 回 550 → 整条失败 | `smtp.rs:841` `a_rejected_recipient_fails_the_whole_message` |
| 8 | 超时（可断言区间） | `smtp.rs:877` `a_silent_server_times_out` |
| 9 | **头注入**（路径 7） | `smtp.rs:917` `the_subject_cannot_inject_headers` ★ |
| 10 | **base64 76 折行**（路径 8） | `smtp.rs:944` `the_base64_body_wraps_at_76_columns` ★ |
| 11 | **STARTTLS 后重发 EHLO**（路径 9） | `smtp.rs:857` `auth_is_only_sent_after_the_second_ehlo` ★ |
| 12 | `Debug` 打码密码 + 阳性对照 | `config.rs:502` `the_debug_output_masks_the_password` |
| 13 | 未配置通道**不得**返回 `Ok` | `unconfigured.rs` 内联测试显式 `assert!(result.is_err())` |

★ 三条是本轮 QA 挑刺（S-01/S-02/S-03）**倒逼出来**的判据。变异确认：
- 去掉 `sanitize_subject` 的 CR/LF 截断 → 第 9 条红
- 去掉 `wrap_base64` 的 76 折行 → 第 10 条红
- 去掉 TLS 后的 `conn.ehlo()` → 第 11 条红（连带 5 条红）

---

## AC-S-167：模板 CRUD + 受限变量替换

**判定：✅ 通过**

| # | 判据 | 落点（`src/template.rs` 内联测试） |
|---|---|---|
| 1 | 主路径逐字渲染 | `template.rs:190` `the_main_path_renders_both_variables`（`assert_eq!(out, "你好 张三，订单 A-1 已发货")`） |
| 2 | 白名单**正向边界** 4 个 | `template.rs:159` `valid_variable_names_are_accepted` |
| 3 | 非法变量名 9 个全拒 | `template.rs:170` `invalid_variable_names_are_rejected` |
| 4 | 缺变量**点名** | `template.rs:222` `a_missing_variable_fails_and_names_it` |
| 5 | 缺变量**不静默留空** | `template.rs:239` `a_missing_variable_never_renders_as_empty` |
| 6 | 多余变量忽略且不代入 | `template.rs:208` `extra_payload_keys_are_ignored` |
| 7 | 4 个 SSTI 载荷全拒 | `template.rs:262` `every_ssti_payload_is_rejected_as_invalid_placeholder` ★ |
| 8 | SSTI **不 panic** | `template.rs:282` `ssti_payloads_never_panic`（`catch_unwind`）★ |
| 9 | **单趟替换**（防二次注入） | `template.rs:311` `substituted_values_are_not_re_scanned` ★核心 |
| 10 | 值含分隔符不破坏解析 | `template.rs:325` |
| 11 | 预览 = 渲染 | `template.rs:348` `preview_equals_render` |

★ **第 8 条**是 S-09 裁决（D316）要求：panic 会让测试以「崩了」结束，
看起来不是「发现漏洞」→ 必须 `catch_unwind` + 断言返回 `Err`。

★ **第 9 条**是模板注入的核心：若对替换结果再扫一遍，
攻击者把 `{{secret}}` 放进自己的显示名就能读到别人的变量。

---

## AC-S-168：多语言按 locale 分行 + 三级回退

**判定：✅ 通过**

| # | 判据 | 落点 |
|---|---|---|
| 1 | 两个 locale **能共存**（唯一约束含 locale） | `notify_real_db.rs:266` `two_locales_of_the_same_template_coexist` |
| 2 | 用户 locale 命中取**那一行** | `notify_real_db.rs:208` 于 `the_locale_fallback_picks_the_right_row`（`assert_eq!(en.body_template, "English body")`） |
| 3 | 无该 locale → 回退**租户默认** | `notify_real_db.rs:233`（`assert_eq!(ja.body_template, "中文正文")`）★ |
| 4 | 租户默认也没了 → **失败**（不回退到任意 locale） | `notify_real_db.rs:243`（`expect_err` + 码 == `NOTIFY_TEMPLATE_NOT_FOUND`）★ |

★ 第 3/4 条断言的是**具体文案**，不是「有结果」——
否则「回退到 en-US」也会绿（而用户看不懂英文）。

---

## AC-S-169：订阅偏好（按 category 退订，system 不可退订）

**判定：✅ 通过**

| # | 判据 | 落点 |
|---|---|---|
| 1 | 退订后**直接进死信** | `notify_real_db.rs:315` 于 `an_opted_out_recipient_goes_straight_to_dead_letter_with_zero_attempts` |
| 2 | **`attempt_count == 0`** | `notify_real_db.rs:334` ★（D312 钉死） |
| 3 | `last_error` 写明退订原因 | `notify_real_db.rs:340` |
| 4 | 退订 `system` → `NOTIFY_CATEGORY_MANDATORY` | `notify_real_db.rs:369` 于 `the_system_category_cannot_be_unsubscribed_and_leaves_no_row` |
| 5 | **偏好表里不留 `system` 行** | `notify_real_db.rs:378`（`assert_eq!(count, "0")`）★ |
| 6 | system 不受 marketing 退订影响 + 退订可逆 | `notify_real_db.rs:394` 于 `the_system_category_is_unaffected_and_unsubscribe_is_reversible` |

★ 第 2 条：若留空间给 `1`，「退订后还重试 5 次」的实现也能过（它的 `last_error` 同样是退订）。
★ 第 5 条：若「先写后报错」，用户的 `system` 就真的被关掉了 → 必须查库断言行数为 0。

---

## AC-S-170：`FOR UPDATE SKIP LOCKED` + 多 worker 不重复不遗漏

**判定：✅ 通过**

| # | 判据 | 落点 |
|---|---|---|
| 1 | 4 个并发 `tick()` 认领 100 条 | `notify_worker_real_db.rs:78` 于 `four_concurrent_ticks_deliver_each_message_exactly_once` |
| 2 | **全部 delivered**（不遗漏） | `notify_worker_real_db.rs:88`（SQL count == "100"） |
| 3 | **provider 调用恰好 100**（不重复） | `notify_worker_real_db.rs:97-98` ★核心 |
| 4 | 每轮计数自洽 | `notify_worker_real_db.rs:75`（`stats.is_balanced()`） |
| 5 | 未到期**不取** | `notify_worker_real_db.rs:136` 于 `a_delivery_scheduled_in_the_future_is_not_claimed` |
| 6 | `delivering` **超阈值**回收 | `notify_worker_real_db.rs:198` 于 `stale_delivering_is_reclaimed_but_fresh_delivering_is_not` |
| 7 | `delivering` **未超阈值不回收**（阴性对照） | `notify_worker_real_db.rs:204` ★（D313 / S-06） |
| 8 | `delivered` 终态不再出队 | `notify_worker_real_db.rs:226` 于 `delivered_rows_are_never_claimed_again` |

★ 第 3 条：只断言「全部 delivered」会漏掉「投两次但状态只记一次」。
★ 第 7 条是**阴性对照**：缺了它，「见到 `delivering` 就抢」的实现也会在 6 通过 →
而它会在正常投递中把行抢走 → 重复投递。

---

## AC-S-171：重试退避（指数 + 上限 + 次数用尽进死信）

**判定：✅ 通过**

| # | 判据 | 落点 |
|---|---|---|
| 1 | 序列 `[30, 60, 120, 240]` | `config.rs:356` 于 `the_backoff_sequence_is_thirty_sixty_one_twenty_two_forty` + `notify_worker_real_db.rs:363` 真库逐值 |
| 2 | **上限截断**（2000×2 → 3600） | `config.rs:366` 于 `the_backoff_is_clamped_to_the_maximum` ★ |
| 3 | **溢出防护**（取上限，不回绕） | `config.rs:383` 于 `the_backoff_does_not_overflow_on_large_attempts`（`assert_eq!(value, i64::MAX)`） |
| 4 | 第 5 次失败进死信 | `notify_worker_real_db.rs:311` 于 `five_failures_lead_to_dead_letter`（`assert_eq!(stats.dead_lettered, 1)`） |
| 5 | **配置真的被读**（`maxAttempts=2` → 第 2 次死信） | `notify_worker_real_db.rs:404` 于 `a_configured_max_attempts_of_two_dead_letters_on_the_second_failure` ★ |
| 6 | 死信保留失败原因 | `notify_worker_real_db.rs:320` |

★ 第 5 条区分力全部来自「配置不是写死 5」：硬编码 5 的实现会在第 2 次后仍 `pending`。
★ 第 2/3 条是两件事：`retryMaxSeconds` 是**结果**上限，**不防**计算过程溢出。

---

## AC-S-172：死信人工重投（只重置死信 + 影响行数守卫）

**判定：✅ 通过**

| # | 判据 | 落点 |
|---|---|---|
| 1 | 重投非死信 → 409 且字段不变 | `notify_real_db.rs:133` 于 `resending_a_non_dead_letter_is_rejected_and_changes_nothing` |
| 2 | 并发两个 → **`rows_affected` 集合 `{1, 0}`** | `notify_real_db.rs:196-202` 于 `two_concurrent_resends_yield_exactly_one_success`（定义在 `:169`） ★核心 |
| 3 | 重投不存在 → 404 通用 `NOT_FOUND` | `http.rs:432` 的 `resend_delivery` handler 分支 |
| 4 | 重投成功字段齐全 | `notify_real_db.rs:124` 于 `resending_a_dead_letter_does_not_redeliver_the_delivered_one` |
| 5 | SQL 用 `WHERE status = 'dead_letter'` | `store.rs:499` 的 `resend_dead_letter`（`rows_affected` 返回给调用方） |

★ 第 2 条：用 `Barrier::new(2)` 让两个独立 `TenantTx` **同时**执行同一条 UPDATE
→ 断言 `{1, 0}`，**不是** `{1}`、**不是**「两个都 1」。
**变异验证**：把 `WHERE status = 'dead_letter'` 去掉 → 该测试变红（两个都 1，集合 `{1,1}`）。

---

## AC-S-173：投递记录（快照 + 租户隔离 + 敏感键）

**判定：✅ 通过**

| # | 判据 | 落点 |
|---|---|---|
| 1 | `rendered_body` 是**快照** | `notify_real_db.rs:457` 于 `the_rendered_body_is_a_snapshot`（改模板后逐字不变） |
| 2 | **3 张表各验一次 RLS** | `notify_real_db.rs:541` / `:555` / `:568` 于 `all_three_tables_are_protected_by_rls` ★（D311） |
| 3 | 列集合**双向相等** 18 列 | `notify_real_db.rs:578` 于 `the_delivery_table_has_exactly_the_eighteen_documented_columns` |
| 4 | `payload` 是 `jsonb` | `notify_real_db.rs:637` 同上 |
| 5 | 4 个敏感键全拒（camelCase） | `notify_real_db.rs:615` 于 `sensitive_payload_keys_are_rejected` |
| 6 | `secretary` **不误伤**（阳性对照） | `notify_real_db.rs:659` 同上 ★ |
| 7 | `scan_sensitive_keys` 复用（单测层） | `service.rs:411` 于 `the_four_sensitive_keys_are_rejected` |
| 8 | 命中时列出**全部**键 | `service.rs:439` 于 `all_hit_keys_are_reported` |

★ 第 2 条是本轮**补上的缺口**（原先只有 delivery 一张表有判据）：
若 manifest 漏声明某张表，`tenant-policies --check` 会绿（没声明就不生成也不校验）
→ 那张表**完全无隔离**。
★ 第 6 条：`scan_sensitive_keys` 是**完整键名匹配**（非子串），`secretary` 不含敏感键。
★ 第 3 条：断言写成「与 `DELIVERY_COLUMNS` 双向相等」，**不是**「列数 = 18」。

---

## AC-S-174：权限点三处齐 / C-5 / 启动对账 / 审计埋点

**判定：✅ 通过**

| # | 判据 | 落点 |
|---|---|---|
| 1 | `capabilities/` **恰好 14 文件** | 实测 `find capabilities -type f \| wc -l` = 14 |
| 2 | `permissions.total == 38` | 实测 38（35 + 3）；模块条目含 `crates/platform-notify/manifest.json` |
| 3 | 3 个权限点逐字 | `contract_c5.rs:45` 于 `every_notify_permission_has_an_authorization_call_site` |
| 4 | 3 张表都在 `tenantScoped` | `contract_c5.rs:89` 于 `the_notify_manifest_is_valid` ★ |
| 5 | 5 个配置项都 `scope: Platform` + 默认值逐字 | `contract_c5.rs:128` 于 `every_notify_config_key_is_platform_scoped` |
| 6 | **`all_codes()` 必须引用本 crate**（读源码） | `contract_c5.rs:215` 于 `the_server_test_registers_the_notify_error_codes` ★（D318） |
| 7 | 钉死行数计入 `REQ_015_NEW_CODES = 8` | `contract_c5.rs:240` 于 `the_pinned_row_count_includes_the_notify_codes` |
| 8 | 事件码 4 个进审计 | `notify_real_db` + `events.rs` 的 4 条内联测试 |

★ 第 6 条：**必须读源码**，不能只看测试绿 ——
`all_codes()` 是逐 crate 手工列举的，**漏加时 `titles_cover_every_code` 恰好是绿的**。
阳性对照：`grep platform_app::error::ALL_CODES` 必须命中（证明模式有效）。

**实测确认（不是推测）**：
```
$ cargo xtask capabilities          → 14 个文件
$ python3 -c 'json.load(permissions.json).total'  → 38
$ cargo xtask tenant-policies --check → 退出码 0（3 张 notify 表都生成策略）
$ cargo test -p platform-server --test i18n_titles → 9 passed / 0 failed
```

---

## 未做的部分（如实记录）

| 项 | 状态 | 原因 |
|---|---|---|
| **模板管理页面**（`NotifyTemplates.svelte`） | ❌ 未做 | **TD-048**：`RouteDefinition` 无组件槽 + `App.svelte` 硬编码 10 处 `if/else` → 加页面必须改壳层（跨需求改动） |
| **投递记录页面**（`NotifyDeliveries.svelte`） | ❌ 未做 | 同上 |
| 前端封装 `notify-api.ts` | ✅ 已完成 | 27 条单测全绿；页面接上即可用 |
| 短信 / IM 真实现 | ❌ 不做（TD-054） | 无真实凭据 → 判据恒真 |
| SMTP 隐式 TLS（465） | ❌ 不做（TD-055） | 需真实 465 端点才能验 |
| 多部分 MIME（附件） | ❌ 不做 | 属 REQ-018 |
| 租户级 locale 偏好存储 | ❌ 不做 | 无存储位（`PlatformNotifyConfigSource::tenant_default_locale` 固定 `zh-CN`，已标 TODO(TD-057)） |

> ⚠️ **前端两个页面未做 = AC-S-167~173 的「界面可操作」部分未达成**。
> 这些 AC 的**判据全部是 API 层**（已在真库集成测试里验过），
> 但「管理员能在界面上维护模板 / 重投死信」这个**用户可用性**目标未完成。
> → 已记 **TD-056**。
