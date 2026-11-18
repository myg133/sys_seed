# C. Feature Flag 与确定性灰度 —— 测试用例（AC-S-056 ~ AC-S-059）

- **需求**: REQ-010「配置、品牌与 Feature Flag」（**rev 2**）
- **来源**: [acceptance.md](../acceptance.md) §AC-S-056 ~ AC-S-059、[design-summary.md](../design-summary.md) §4.1 / §5 / §5.3、[api-conventions v2 §C10](../../../contracts/api-conventions/v2.md)
- **用例数**: 12（TC-010-020 ~ TC-010-031）
- **公共夹具**: 见 [index.md](index.md) §3 测试夹具

## C.0 本域约定（rev 2）

- **接口**（design §6）：`GET /api/feature-flags`、`GET /api/feature-flags/{flag}`、`PUT /api/feature-flags/{flag}`（权限点 `config:flag:write`）。读接口**需要令牌**。
- **响应形状**：单条 `{"flag","enabled","source"}`（C10）；批量 `{"flags":[...]}`。字段名是 `flag`。
- **灰度公式**（C10 v2 §10.2）：`bucket = sha256("<登记名>:<user_id>")` 前 4 字节按**大端**解释为 `u32`；`enabled = bucket % 100 < rolloutPercent`。
- **`rolloutPercent` 用派生键**（D57）：对每个已登记的 flag `F`，平台自动认为存在
  `flags.<F>.rolloutPercent`（`type: number` / `scope: tenant` / `0..=100` / 默认 `100`）。
  - 它不是 manifest 里的条目，但**视为已声明**（因此写它不会触发「未声明 → 400」）。
  - `flags.nosuch.rolloutPercent`（flag 未登记）→ **400 `CONFIG_KEY_NOT_REGISTERED`**。
  - flag 本体仍是 manifest 的 `type: boolean` 键，语义是「总开关」：本体 `false` 时**不看比例**。
  - ⚠️ **授权边界未定义**：flag 本体与派生键都是普通 `config` 键，`PUT /api/config/entries` 只要求 `config:entry:write` —— 只有 entry 写权限的用户可能绕过 `config:flag:write`（§8.9 N14）。本域写者 A 同时持有 entry + flag 权限点，不触发该缺口；但该缺口需要 BA 补一条 AC。
- **flag 名精确匹配、不归一化**（D65）：`newInvoice` 与 `new-invoice` 是**两个不同** flag；哈希输入用**登记名原文**。
- **flag 名规范**：`^[a-z][a-zA-Z0-9]{0,63}$`；非法名 → **400 `FLAG_NAME_INVALID`**，**不进日志**。
- **`source` 取值**：显式写在租户级 → `tenant`；未配（用 manifest 默认）→ **`default`**（AC-S-056 步骤 2 写死）；未登记 flag → `default`（§5.3）。
  ⚠️ design §5.1 仍写「无任何配置 → `source="platform"`」，与 AC-S-056 / §5.3 矛盾 —— 见 [index.md](index.md) §8.9 N8；本域按 **AC** 断言 `default`。
- **执行位置**：单用户/单 flag 用**真实库 + 真机 HTTP**；200 用户批量判定用**单元层**（造 200 个令牌不现实）；跨进程用**两个独立进程**。

---

### TC-010-020: flag 按租户隔离；关闭返回 200 + `enabled=false`；无令牌 401
- **关联 AC**: AC-S-056
- **前置条件**:
  - **执行位置**: 真实库 + 真机 HTTP。
  - `newInvoice` 已登记（`type: boolean`，默认 `false`，`scope: tenant`）；**T1 租户级 `true`；T2 无配置**（§3.6）。
  - 读 T1 用 `u-2001`；读 T2 用 `u-2005`。
- **测试步骤**:
  1. `GET /api/feature-flags/newInvoice` + `u-2001` + `X-Tenant-Id: tenant-a`
  2. 同请求改带 `u-2005` + `X-Tenant-Id: tenant-b`
  3. 同请求，**不带** `Authorization`
  4. A（`u-2002`）把 T1 的 `newInvoice` 改成 `false`，再以 `u-2005` 读 T2，然后把 T1 改回 `true`
- **预期结果**:
  1. **200**；`enabled == true`；`source == "tenant"`（**不带"若"** —— 前置已写死配在租户级）。
  2. **200**；`enabled == false`；`source == "default"`（T2 未配 → 用 manifest 默认值）；**不是** 404。
  3. **401**；`code == "UNAUTHENTICATED"`；**不得** 200 + `enabled=false`（无身份只在单元层验，见 TC-010-021）。
  4. 改 T1 后：T2 仍 `false`/`default`（不受影响）；T1 读 `false`；改回后 T1 恢复 `true`/`tenant`。
  5. 全程 T1/T2 的取值互不影响。
- **类型**: 功能 / 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 判定表（租户 × true/false/未配）+ 跨租对照 + 契约 C10

### TC-010-021: 无用户身份时灰度 fail closed（单元层）
- **关联 AC**: AC-S-058
- **前置条件**:
  - **执行位置**: 真实库 + 单元层 + 真机 HTTP。
  - `beta` 已登记；派生键 `flags.beta.rolloutPercent = 100`（「全都命中」的极端值）。
- **测试步骤**:
  1. 单元层：`resolver.flag(&RequestContext::new(), "beta")`（无 user、无 tenant）
  2. 单元层：`resolver.flag(&RequestContext::new().with_tenant("tenant-a"), "beta")`（有租户、**无 user**）
  3. 单元层：`resolver.flag(&ctx.with_user("u-3001"), "beta")`（有 user，对照）
  4. HTTP：不带令牌 `GET /api/feature-flags/beta`
- **预期结果**:
  1. `enabled == false`（**不是** `true`，即使 `p=100`）。
  2. `enabled == false` —— 缺的是**用户身份**，不是租户。
  3. `enabled == true` —— 有身份 + `p=100` 必须命中（证明第 1/2 步不是"实现永远返回 false"）。
  4. **401**；`code == "UNAUTHENTICATED"`（AC-S-058：HTTP 层断言 401，**不是** 200）。
  5. 结论：`p=100` 不改变「无身份 → false」；缺的不是比例而是身份。
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: fail closed + 边界（`p=100` 不改变结论）+ 正向对照

### TC-010-022: 有身份 + `p=100` → true；`p=0` → false；三档在「有/无身份」下逐一对照
- **关联 AC**: AC-S-058
- **前置条件**:
  - **执行位置**: 真实库 + 单元层（`p` 由测试逐次写派生键）。
  - 用户 `u-3001`（T1）。
- **测试步骤**:
  1. 以 `ctx(user=u-3001, tenant=tenant-a)`：`p=100` → 读；`p=0` → 读；`p=50` → 读
  2. 以 `ctx(tenant=tenant-a)`（无 user）：同样三档
  3. 以 `ctx()`（无 tenant、无 user）：同样三档
- **预期结果**:
  1. `100 → true`；`0 → false`；`50 → 记录该用户的布尔值`（用于 TC-010-023）。
  2. 三档**全部 `false`**。
  3. 三档**全部 `false`**。
  4. 第 1 步的三档**不全相同**（否则比例参数没生效）；第 2/3 步三档**全相同**（恒 false）。
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 三档 × 三种身份状态的判定表

### TC-010-023: 确定性：同一用户连读 10 次一致；派生键就是比例的唯一来源
- **关联 AC**: AC-S-057
- **前置条件**:
  - **执行位置**: 真实库 + 真机 HTTP。
  - `beta` 的派生键 `flags.beta.rolloutPercent = 50`（用 `PUT /api/config/entries` 写，`scope: tenant`）；用户 `u-3001`。
- **测试步骤**:
  1. 用 `u-3001` 的令牌连续 `GET /api/feature-flags/beta` **10 次**（间隔 < 100ms）
  2. 记录每次的 `enabled`
  3. 再连续读 10 次（共 20 次）
  4. 用 `PUT /api/feature-flags/beta` 设 `{"enabled":true,"rolloutPercent":50}`，重读 `flags.beta.rolloutPercent` 的值
  5. 尝试写 `flags.nosuch.rolloutPercent = 50`
- **预期结果**:
  1. 20 次的 `enabled` **完全相同**（全 `true` 或全 `false`）；中途翻转即判失败。
  2. 20 次响应 `flag == "beta"`、状态码全 **200**。
  3. 第 4 步：派生键的值仍为 **50** —— flag 写接口与派生键是**同一条数据**，不是两套存储。
  4. 第 5 步：**400**；`code == "CONFIG_KEY_NOT_REGISTERED"`（派生键只对**已登记**的 flag 存在）。
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 重复执行 + 双通道一致性（flag 接口 ↔ 派生键）+ 派生边界

### TC-010-024: 分布落在 [70,130]，命中集合被记录且可复现
- **关联 AC**: AC-S-057
- **前置条件**:
  - **执行位置**: 真实库 + 单元层（200 个用户逐个判定）。
  - 用户 `u-3001` ~ `u-3200`（T1）；`flags.beta.rolloutPercent = 50`。
- **测试步骤**:
  1. 逐个 `resolver.flag(&ctx(user=u-300i, tenant=tenant-a), "beta")`
  2. 统计命中数 `H50` 与命中集合 `S50`（按 user_id 字典序）
  3. 把 `S50` 的 sha256 指纹写进测试输出
- **预期结果**:
  1. `70 ≤ H50 ≤ 130`（AC-S-057 写死的接受区间）。
  2. `H50 ∉ {0, 200}`。
  3. `S50` 被**逐条记录**（200 个 id 的集合，不是只记数量）；指纹可复现。
  4. 连续跑两次该步骤，`S50` 与指纹**完全相同**。
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 统计边界 + 可复现集合

### TC-010-025: 单调性：`S50 ⊊ S60`、`S50' == S50`、反向 60→50 为子集
- **关联 AC**: AC-S-057
- **前置条件**:
  - **执行位置**: 真实库 + 单元层。
  - 同 TC-010-024 的 200 个用户。
- **测试步骤**:
  1. `p=50` → 记录 `S50`
  2. `p=60` → 记录 `S60`
  3. 再 `p=50` → 记录 `S50'`
  4. 从 `p=60` 改成 `p=50` → 记录 `S50''`
  5. `p=10` → 记录 `S10`
- **预期结果**:
  1. `S50 ⊆ S60`；且 `|S60| > |S50|`（AC 写的是 **`S50 ⊊ S60`** 严格包含；n=200 时该差值成立的期望概率 > 0.999 999）。
  2. `S50' == S50`（改回原比例，集合**完全复原** —— 证明只依赖 `(flag, user)` 与阈值，不依赖调用顺序）。
  3. `S50'' == S50`（第 4 步：反向调低后集合只能是原子集）。
  4. `S10 ⊆ S50`，且 `|S10| ≤ |S50|`。
  5. 出现"调高比例后原命中者掉出"即判失败。
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 集合包含关系（正向严格包含 + 复原 + 反向 + 更小比例）

### TC-010-026: 边界 `p=0` 与 `p=100`
- **关联 AC**: AC-S-057
- **前置条件**:
  - **执行位置**: 真实库 + 单元层。
- **测试步骤**:
  1. `p=0`，对 200 个用户判定
  2. `p=100`，对 200 个用户判定
- **预期结果**:
  1. `p=0` → 命中数 **0**；每个用户的判定仍是 `enabled=false`（响应结构不变）。
  2. `p=100` → 命中数 **200**。
  3. 两次判定的响应结构与 `p=50` 时**完全一致**（只有 `enabled` 变化）。
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 边界值（下界 / 上界）

### TC-010-027: 非法比例：`101` / `-1` / `50.5` 在写时 400；脏数据读时 fail closed
- **关联 AC**: AC-S-057
- **前置条件**:
  - **执行位置**: 真实库 + 真机 HTTP + 管理员连接。
  - 写者 A（`config:flag:write`）。
- **测试步骤**:
  1. A：`PUT /api/feature-flags/beta` body `{"enabled":true,"rolloutPercent":101}`
  2. 同 body `rolloutPercent: -1`
  3. 同 body `rolloutPercent: 50.5`
  4. A：经 `PUT /api/config/entries` 写派生键 `flags.beta.rolloutPercent = 101`（`scope: tenant`）
  5. 用**管理员**连接把存储值改成 `101`，**同时把 `'tenant:tenant-a'` 的 epoch +1**（模拟一次真实写入路径产生的脏数据；只改值不改 epoch 的话，读路径会命中旧缓存，验不到「脏数据读时 fail closed」），再以 `u-3001` 读 `beta`
  6. 恢复 `p=50`
- **预期结果**:
  1. **400**；`code == "CONFIG_VALUE_OUT_OF_RANGE"`；`params` 给出允许范围 `0..=100`；存储值**未被修改**。
  2. **400**；同码。
  3. **400**；同码（整数百分比）。
  4. **400**；`code == "CONFIG_VALUE_OUT_OF_RANGE"` —— 派生键与 flag 接口走**同一套范围校验**。
  5. **200**；`enabled == false`（**fail closed**）—— **不是** 400/500：读它的客户端没有发送非法值；同时产生 **1 条 WARN**，含 `beta` 与「rollout 非法」。
  6. **200**；恢复正常判定。
  7. 判据：**400 固定在 PUT 写入侧**（AC-S-057 步骤 6 明写），读路径对脏数据 fail closed。
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 边界（0/100 之外）+ 双写入口 + 脏数据 fail closed

### TC-010-028: 两个独立进程一致 + 逐用户核对哈希公式
- **关联 AC**: AC-S-057
- **前置条件**:
  - **执行位置**: 真实库 + **两个独立进程**（各自启动、各自持缓存、共享同一 `base_test`）。
  - `p=50`；用户 `u-3001` ~ `u-3200`。
- **测试步骤**:
  1. 进程 1 对 200 个用户判定 → `S1`
  2. 进程 2 对同一批用户判定 → `S2`
  3. 断言 `S1 == S2`
  4. 对 `u-3001` ~ `u-3005`，在测试代码里**独立**计算：`b = u32::from_be_bytes(sha256("beta:" + user_id)[0..4])`，`expected = b % 100 < 50`
  5. 断言实现返回的 `enabled` 与第 4 步的 `expected` 逐条相等
- **预期结果**:
  1. `S1 == S2`（集合完全相等，与顺序无关）—— **不得**出现"两个进程各命中一半"。
  2. 5 个样本用户与独立公式**逐条一致**。
  3. 独立计算**不得**调用被测实现（不能复用它的哈希/桶函数），否则公式抄错也会通过。
  4. 若用了 `RandomState` 或其他随机播种哈希，第 1 步或第 3 步必然失败。
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 跨进程一致性（真进程）+ 独立实现比对

### TC-010-029: 未声明的 flag → 200 + false + `source=default` + 一条结构化 WARN
- **关联 AC**: AC-S-059
- **前置条件**:
  - **执行位置**: 真实库 + 真机 HTTP + 日志捕获（`tracing` 订阅器写内存缓冲）。
  - manifest 登记 `newInvoice` 与 `beta`（见 §8.9 N11：AC-S-059 的前置只写 `newInvoice`，与 AC-S-057 需要 `beta` 互斥 —— 本用例集两者都登记）。
- **测试步骤**:
  1. 以 `u-2001` 读 `GET /api/feature-flags/newinvoce`（拼错一个字母）
  2. 取出捕获到的日志事件
  3. 对照：以 `u-2001` 在 T1 读 `GET /api/feature-flags/newInvoice`
- **预期结果**:
  1. **200**；`enabled == false`；`source == "default"`；`flag == "newinvoce"`；**不是** 404、**不是** 500。
  2. 恰好 **1 条** `WARN` 事件，其结构化字段 `flag == "newinvoce"`（**字段截断**，不是把原文拼进整行）；日志文本不得被它渲染成非法 JSON。
  3. 对照读 `newInvoice` → **200 + `enabled=true`**（证明不是"所有 flag 都返回 false"）。
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 正常/异常对照 + 日志结构性断言

### TC-010-030: flag 名边界：非法名 400 不进日志；kebab 不做归一化
- **关联 AC**: AC-S-059
- **前置条件**:
  - **执行位置**: 真实库 + 真机 HTTP + 日志捕获。
  - `newInvoice` 在 T1 为 `true`（§3.6）。
- **测试步骤**:
  1. 读 `bad\nINJECTED`（含换行）
  2. 读 1024 个 `a` 组成的名字
  3. 取捕获日志，确认这两个请求**没有**产生任何 `flag=` 的 WARN 事件
  4. 读 `newinvoice`（**全小写、符合字符集**、但 ≠ 登记的 `newInvoice`）
  5. 对照读 `newInvoice`
- **预期结果**:
  1. **400**；`code == "FLAG_NAME_INVALID"`（**不是** 200 + false）。
  2. **400**；同码（超长）。
  3. 步骤 1/2 **没有**产生日志事件 —— 非法名**不进日志**，因此无法伪造日志行/刷爆告警。
  4. **200**；`enabled == false`；`source == "default"` —— `newinvoice` 与 `newInvoice` 是**两个不同** flag，**不做归一化**。
     另：kebab 写法 `new-invoice` 含连字符 → 按 D77 是**非法名**，应得 **400 `FLAG_NAME_INVALID`**（不是 200）。
  5. **200**；`enabled == true`（对照：拼写不同的合法名得到不同结果）。
  6. 全程不得 500、不得 panic。
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 攻击面（日志注入 / DoS）+ 合法名边界 + 不归一化判别

### TC-010-031: 空间级不得影响 flag（未校验的 `X-Space-Id` 不能改变行为）
- **关联 AC**: AC-S-056
- **前置条件**:
  - **执行位置**: 真实库 + 真机 HTTP。
  - `beta` 登记 `scope: tenant`；派生键 `flags.beta.rolloutPercent` 也是 `scope: tenant`。
  - 写者 A。
- **测试步骤**:
  1. A：`PUT /api/feature-flags/beta` `{"enabled":true}` + `X-Space-Id: space-sales-1`
  2. A：`PUT /api/config/entries` `{"scope":"space","entries":[{"key":"flags.beta.rolloutPercent","value":50}]}` + S1
  3. 读 `beta`：T1（带 `X-Space-Id: space-sales-1`）与 T1（不带空间头）各一次
  4. 管理员连接查 `platform_space_config` 中 `flag`/派生键的行数
- **预期结果**:
  1. 第 1、2 步都是 **400**；`code == "CONFIG_SCOPE_NOT_ALLOWED"`（flag 与派生键的允许最深层都是 `tenant`；design §4.1 另有「Feature Flag 不支持空间级」——两条规则同向）。
  2. 两次读取结果**完全相同**（`X-Space-Id` 不改变判定）。
  3. `platform_space_config` 中 `key='beta'` 与 `key='flags.beta.rolloutPercent'` 的行数 **均为 0**。
  4. 结论：未校验的 `X-Space-Id` 不得改变开关行为（design §4.1 / R1）。
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 攻击面（未校验头）+ 库侧证据

---
