# REQ-022「出站 IdP 与单点登出」—— 测试用例总索引

- **需求编号**: REQ-022
- **父需求**: REQ-001（通用项目底座）｜**承接父 AC**: AC-006（底座作为 IdP 向第三方应用签发令牌）
- **验收基线**: [acceptance.md](../acceptance.md) rev 1 —— **22 条 AC**（AC-S-134 ~ AC-S-155）
- **设计依据**: [design-summary.md](../design-summary.md) rev 1（D196~D220）
- **需求原文**: [demand.md](../demand.md) rev 2
- **契约依据**: `BA/contracts/api-conventions/v3.md`（C1 新增 `/api/oauth` + **C14** 协议端点错误体例外）
- **代码基线**: `code/` @ `develop` @ `310e4d3` —— **本需求尚未实现**
- **状态**: 待实现（用例本身**尚未执行**，不得据此判定需求通过）
- **版本**: 第 1 版（rev 1）—— QA 逐条核对 acceptance rev 1 与 `code` 源码 + `base_test` **只读实测**
- **配套挑刺报告**: [qa-req022-spec-review.md](../../../../feature-meta/qa-req022-spec-review.md)
  （**39 条**挑刺，其中 **10 条阻塞**；**22 条 AC 全部有问题**）

> ⚠️ **本索引 §1.2 与配套的挑刺报告是同一轮产出**：
> 用例按「**可机械判定**」写（已对 AC 的不可执行处逐条标注 `〔S-xx〕` 与 `〔AC 修正〕`），
> 最危险的三条见 §1.2，全部 39 条的逐条证据见报告。

---

## 1. 概览

| 项 | 值 |
|---|---|
| 用例总数 | **131**（TC-022-001 ~ TC-022-131，编号连续、无跳号、无重复 —— 已用脚本核对，见 §4） |
| 覆盖 AC | **22 / 22**（无遗漏、无多余、无悬空 TC） |
| 用例文件 | **12 个**（另本索引 1 个） |
| 类型分布 | 安全 **66** / 功能 **41** / 边界 **14** / 可维护性 **10** |
| 优先级分布 | P0 **109** / P1 **22** / P2 **0** |
| 自动化分布 | 可自动化 **128** / 半自动 **3** / 手工 **0** |
| 标注了挑刺编号的用例 | **53 条**（分布在 12 个文件里，见 §6） |
| 标注了 `〔AC 修正〕` 的用例 | **33 条** |
| 挑刺编号被用例覆盖 | **39 / 39**（S-01 ~ S-39 **每一条**都有对应用例） |
| 每条用例字段 | 关联 AC / 前置条件 / 测试步骤 / 预期结果 / 类型 / 优先级 / 自动化 / 设计方法 / 备注 |

### 1.1 为什么「手工 0 条」

testing.md 的测试预算原则是「**低风险**项才允许手工验证 / 暂不自动化」。
REQ-022 里**没有低风险 AC**：22 条 AC 全部带安全或登记断言
（secret 只存摘要、PKCE 强制 S256、scope 不得越过用户本人、
令牌只在内省端点可用、底座业务端点默认拒绝、事件码不带明文、跨租户不可见……）。
把其中任何一条降级成手工，就等于把「忘了跑也没人知道」变成默认状态。

预算纪律在本轮是这样落实的：

- **3 条**判据本身不是断言式的（起进程看 fail-fast、两次「还原 + 重跑 + 比 `sha256`」的变异验证）
  → 标 **半自动**并写明理由，**不硬塞进常规套件**：
  | 用例 | 为什么是半自动 |
  |---|---|
  | TC-022-115 | 要**手动改 manifest** 制造不一致、再手动起进程看退出码与日志 |
  | TC-022-118 | 要**改源码**做两次反向变异、每次**还原 + `touch`**（dev-guide §9.4） |
  | TC-022-038 | 要改 `oauth.authorizationCodeTtlSeconds` 配置并**重启**服务 |
- **边界 / 阳性对照 / 反向变异**按风险加，不按覆盖率凑数：
  22 条 AC 里，**9 条**用例是「变异 / 篡改 / 对照组」类 —— 因为「按错的规格实现 + 按错的规格验收」
  是自洽闭环，**只有变异能证明判据在跑**（dev-guide §9.13）。

### 1.2 本轮最危险的三个结论（先看这三条）

1. **未登录时，OAuth 的 query 参数被前端路由**丢弃** —— 整个授权码流程**物理不可达**（挑刺 **S-02**，阻塞）。
   `packages/auth/src/index.ts:118-132` 的 `guard()` 只给 `'/login'`（**没有** `return_to`）；
   `apps/shell/src/lib/router.ts:229-242` 的 `write()` 在 **path 变化时丢弃 `search`**；
   `router.ts:261` 的 `intendedPath` **只有 pathname**；
   `App.svelte:120-125` 登录后跳的就是那个**不带 query** 的 `intendedPath`。
   → 照现状走一遍授权流程，**登录完 `client_id` / `state` / `code_challenge` / `redirect_uri` 全没了**。
   → AC-S-137（7 条）+ AC-S-138（5 条）+ AC-S-148（3 条）**共 15 条用例全部悬空**。
   → **BA 必须先选方案**（公开路由 + 登录页 `return_to` / 改 `router.ts` 的 `write()`）。

2. **第三方回调 `https://client.example/cb` 在 E2E 里**不可达** —— AC 的「读地址栏的 `code=`」这一步**按字面执行不了**（挑刺 **S-01**，阻塞）。
   design §4.1 写明 `redirectTo` 由**前端**执行 `location.href` 跳转 → 浏览器会真的导航到那个主机；
   `playwright.config.ts:3-11` 又写明本仓**没有 CORS 层**、E2E 走**同源**。
   → Playwright 里 DNS 失败的顶层导航会抛错，**地址栏停在跳转前**，`page.url()` 拿不到 `code`。
   → **可执行的替代判据**：用 `page.route('https://client.example/**', route => route.fulfill(...))`
   **拦截**（仓里的既有范式：`e2e/sso.spec.ts:43-50`），拦截后地址栏**就是**目标 URL → 可读。

3. **D215 的 8 个 `oauth.*` 事件码进 `TITLES` → 会让 `i18n_titles.rs` 的三个既有测试**必红**（挑刺 **S-04**，阻塞）。
   `codes_follow_their_namespace_convention`（`:285-300`）只允许 `UPPER_SNAKE` **或** `org.` 前缀 →
   `oauth.client.registered` **两者都不满足**；
   `every_error_code_is_registered_in_every_locale`（`:357-405`）只排除来源标签 `platform-org::notify` →
   任何**新增来源**都会被算成「错误码」→ 要求 i18n 有 `error.oauth.*` → **与 D215「事件码不进 `error.*`」直接冲突**；
   `no_orphan_titles`（`:190-202`）又要求事件码被某条 `ALL_CODES` 认领 → 又触发上一条。
   → **BA 二选一**：事件码**不进** `TITLES`（→ **94** 行）或**进**并同步改 `i18n_titles.rs` 三处（→ **102** 行）。
   → 顺带：AC-S-152 的「基线 58 + 21 + 5 + **本需求新增数**」**不是一个数**（挑刺 **S-03**）→ 必须写死。

---

## 2. 文件清单与 AC 映射

| 文件 | 能力域 | AC 范围 | 用例数 | 编号段 |
|---|---|---|---|---|
| [metadata.md](metadata.md) | 授权服务器元数据（RFC 8414） | AC-S-134 | **8** | 001 ~ 008 |
| [client.md](client.md) | client 管理 + secret 只存摘要 | AC-S-135、AC-S-136 | **14** | 009 ~ 022 |
| [authorization-code.md](authorization-code.md) | 授权码 + PKCE（端到端 / 强制 S256 / 一次性） | AC-S-137、AC-S-138、AC-S-139 | **16** | 023 ~ 038 |
| [pkce-scope.md](pkce-scope.md) | scope 三重交集 / 通配 / 不得越过用户本人 | AC-S-140 | **8** | 039 ~ 046 |
| [client-credentials.md](client-credentials.md) | Client Credentials（无用户令牌） | AC-S-141 | **6** | 047 ~ 052 |
| [device-code.md](device-code.md) | Device Code（RFC 8628，含 `slow_down`） | AC-S-142 | **10** | 053 ~ 062 |
| [introspect-revoke.md](introspect-revoke.md) | 令牌内省（RFC 7662）+ 撤销（RFC 7009） | AC-S-143、AC-S-144 | **14** | 063 ~ 076 |
| [consent-grant.md](consent-grant.md) | 同意页 / 授权记录 / 免登 | AC-S-146、AC-S-147、AC-S-148 | **14** | 077 ~ 090 |
| [token-format.md](token-format.md) | 令牌格式与第三方验签（ES256 / JWKS） | AC-S-145 | **7** | 125 ~ 131 |
| [logout.md](logout.md) | Back-Channel / Front-Channel / 按应用登出 | AC-S-149、AC-S-150 | **12** | 091 ~ 102 |
| [default-deny.md](default-deny.md) | 底座业务端点默认拒绝 OAuth 令牌 | AC-S-151 | **6** | 103 ~ 108 |
| [registration.md](registration.md) | 错误码登记 / 权限点 / 审计 / 租户隔离 | AC-S-152、AC-S-153、AC-S-154、AC-S-155 | **16** | 109 ~ 124 |
| **合计** | — | **22 条 AC** | **131** | 001 ~ 131 |

> 编号段**不按文件顺序连续**（`token-format.md` 排在 `consent-grant.md` 之后是按**依赖**排的：
> 验签用例要用到同意流程签出来的令牌）→ 编号的连续性由 §4 的脚本核对保证。

---

## 3. 追溯矩阵（AC → TC）

| AC | 主题 | 用例编号 | 用例数 | 其中 P0 | 悬空 TC |
|---|---|---|---|---|---|
| AC-S-134 | 授权服务器元数据可被发现 | TC-022-001 ~ 008 | **8** | 7 | 0 |
| AC-S-135 | client 可注册 / 改 / 停用 / 删 + 回调白名单 | TC-022-009 ~ 016 | **8** | 7 | 0 |
| AC-S-136 | secret 只存摘要 + 任何读接口不回显 | TC-022-017 ~ 022 | **6** | 5 | 0 |
| AC-S-137 | 授权码 + PKCE 端到端换到可用令牌 | TC-022-023 ~ 029 | **7** | 7 | 0 |
| AC-S-138 | PKCE 强制 S256 | TC-022-030 ~ 034 | **5** | 5 | 0 |
| AC-S-139 | 授权码一次性 / 只存摘要 / 60 秒过期 | TC-022-035 ~ 038 | **4** | 3 | 0 |
| AC-S-140 | scope 三重交集 / 通配 / 不越过用户本人 | TC-022-039 ~ 046 | **8** | 6 | 0 |
| AC-S-141 | Client Credentials 换到无用户令牌 | TC-022-047 ~ 052 | **6** | 5 | 0 |
| AC-S-142 | Device Code 全流程 | TC-022-053 ~ 062 | **10** | 8 | 0 |
| AC-S-143 | 令牌内省（只内省自己的） | TC-022-063 ~ 070 | **8** | 7 | 0 |
| AC-S-144 | 令牌可撤销 + grant 级撤销 | TC-022-071 ~ 076 | **6** | 5 | 0 |
| AC-S-145 | 令牌是 ES256 JWT，第三方可自行验签 | TC-022-125 ~ 131 | **7** | 6 | 0 |
| AC-S-146 | 同意页 + 同意记录落库 + 用户所见 = 实际授予 | TC-022-077 ~ 083 | **7** | 6 | 0 |
| AC-S-147 | 我的授权可查可撤销 + 令牌立即失效 | TC-022-084 ~ 087 | **4** | 4 | 0 |
| AC-S-148 | 已有授权时免登（静默续发） | TC-022-088 ~ 090 | **3** | 3 | 0 |
| AC-S-149 | Back-Channel Logout 投递 `logout_token` | TC-022-091 ~ 097、102 | **8** | 7 | 0 |
| AC-S-150 | Front-Channel Logout + 按应用登出 | TC-022-098 ~ 101 | **4** | 4 | 0 |
| AC-S-151 | 底座业务端点默认拒绝 OAuth 令牌 | TC-022-103 ~ 108 | **6** | 5 | 0 |
| AC-S-152 | 错误码 8 处登记 + 跨栈「包含」 | TC-022-109 ~ 113 | **5** | 5 | 0 |
| AC-S-153 | 权限点三处齐 + 启动对账 fail-fast + C-5 | TC-022-114 ~ 118 | **5** | 5 | 0 |
| AC-S-154 | 关键动作产生审计记录 | TC-022-119 ~ 121 | **3** | 3 | 0 |
| AC-S-155 | OAuth 数据跨租户不可见 | TC-022-122 ~ 124 | **3** | 3 | 0 |
| **合计** | — | **TC-022-001 ~ 131** | **131** | **109** | **0** |

---

## 4. 编号连续性核对（**已用脚本数过**）

```bash
cd BA/demands/REQ-022-outbound-idp/test-cases

# ① 编号连续、无跳号、无重复
grep -h '^### TC-022-' *.md | sed -E 's/^### (TC-022-[0-9]+).*/\1/' | sort > /tmp/got.txt
seq -f "TC-022-%03g" 1 131 > /tmp/want.txt
diff /tmp/want.txt /tmp/got.txt        # 必须无输出
grep -h '^### TC-022-' *.md | sed -E 's/^### (TC-022-[0-9]+).*/\1/' | sort | uniq -d   # 必须无输出

# ② 覆盖恰好 22 条 AC、无悬空
grep -h '^- \*\*关联 AC\*\*:' *.md | sed -E 's/.*: (AC-S-[0-9]+).*/\1/' | sort -u | wc -l   # → 22
grep -h '^- \*\*关联 AC\*\*:' *.md | sed -E 's/.*: (AC-S-[0-9]+).*/\1/' | sort -u \
  | diff <(seq -f "AC-S-%03g" 134 155) -                                                # 必须无输出

# ③ 每条用例的 9 个字段齐全
for f in *.md; do
  n=$(grep -c '^### TC-022-' "$f")
  for k in '关联 AC' '前置条件' '测试步骤' '预期结果' '类型' '优先级' '自动化' '设计方法' '备注'; do
    c=$(grep -c "^- \*\*$k\*\*:" "$f")
    [ "$c" -ge "$n" ] || echo "✗ $f 缺 $k（$c/$n）"
  done
done

# ④ 挑刺编号全部被用例引用（S-01 ~ S-39）
grep -ho '〔S-[0-9][0-9]〕' *.md | sort -u | sed -E 's/〔(S-[0-9][0-9])〕/\1/' \
  | diff <(seq -f "S-%02g" 1 39) -                                                      # 必须无输出
```

**本轮实测结果**：① `diff` 无输出、`uniq -d` 无输出；② 输出 **22**，且与 `AC-S-134 ~ AC-S-155` **逐字相等**；
③ **无缺字段**；④ **S-01 ~ S-39 全部命中**。

---

## 5. 夹具清单

> **原则 1**：造数走管理员连接（`base`，表 owner + 超管），业务访问走应用连接（`base_app`）；
> 绝不写 `base_dev`（acceptance 全局约定第 4 行）。
> **原则 2**：夹具走**独立、幂等、可复现**的播种脚本（`scripts/seed-e2e.sh`），不依赖其它测试的副作用。
> **原则 3**：E2E 的 `skip` **只允许「被测系统未就绪」**；夹具 / 授权不对必须 **fail**（D108 / dev-guide §9.9）。
> **原则 4**：本轮**未执行任何用例**（需求尚未实现）—— 用例是**给 Dev 的输入**，不是验收结论。
> **原则 5**：**每个夹具都要写清「谁建」** —— 见 §5.3 的「播种方」列。

### 5.1 租户与账号

| 标识 | 租户 | 权限点 | 用途 | 播种方 |
|---|---|---|---|---|
| `u-perm-read@example.com` | `tenant-a` | `rbac:role:read` + `sandbox:demo:read` | 只读用户（AC-S-137/140 的主体） | **既有** `seed-e2e.sh` |
| `u-perm-write@example.com` | `tenant-a` | `rbac:role:read` + `rbac:role:write` + `sandbox:demo:*` | 有 write 的对照（AC-S-140 路径 4） | **既有** `seed-e2e.sh` |
| `u-perm-none@example.com` | `tenant-a` | （无） | fail closed 对照 | **既有** `seed-e2e.sh` |
| `u-perm-audit@example.com` | `tenant-a` | `audit:events:read` | 审计端点对照（AC-S-151） | **既有** `seed-e2e.sh` |
| **`u-perm-oauth@example.com`** | `tenant-a` | `oauth:client:read` / `oauth:client:write` / `oauth:client:delete` | **client 管理端点的操作者** | ⚠️ **本需求新增**（S-06） |
| **`u-perm-other@example.com`** | **`tenant-b`** | `rbac:role:read` | **跨租户对照**（AC-S-147 步骤 5 / AC-S-155） | ⚠️ **本需求新增**（S-34） |

### 5.2 client 夹具（**全部由 `scripts/seed-e2e.sh` 播种**，明文 secret 写到 `target/e2e-oauth-clients.json`）

| 标识 | type | redirect_uris | allowed_grant_types | allowed_scopes | logout URI |
|---|---|---|---|---|---|
| `client-A` | confidential | `https://client.example/cb` | authorization_code / client_credentials / device_code | `rbac:role:read` `rbac:role:write` | backchannel=`http://127.0.0.1:$MOCK_PORT/logout`；frontchannel=`https://client.example/logout` |
| `client-B` | confidential | `https://client.example/cb2` | authorization_code / client_credentials | `sandbox:demo:read` | 无 |
| `client-public` | public | `https://client.example/cb4` | authorization_code / device_code | `sandbox:demo:read` | 无（**无 secret**） |
| `client-disabled` | confidential | `https://client.example/cb5` | authorization_code | `rbac:role:read` | 无（`status='disabled'`） |

> ⚠️ `client-A` 的 `backchannel_logout_uri` 里的 `$MOCK_PORT` 是**动态**的 →
> 播种脚本必须**先起 mock、再写 client 行**（S-07）。

### 5.3 其它夹具

| 项 | 说明 | 播种方 |
|---|---|---|
| **mock 接收端** | `scripts/mock-logout-receiver.sh`（**独立进程**），监听 `127.0.0.1:$MOCK_PORT`；收到的 `logout_token` 追加写 `target/mock-logout-received.jsonl`；行为由 `MOCK_MODE=ok\|timeout\|500` 控制 | ⚠️ **本需求新增**（S-07） |
| **JWT 私钥**（给 TC-022-080 用） | `target/e2e-jwt-private.pem`（0600、不入库、每次运行覆盖） | ⚠️ **本需求新增**（S-08） |
| **E2E 拦截** | `page.route('https://client.example/**', …)` —— 拦截第三方回调 | 测试内（TC-022-026） |

### 5.4 连接角色（**关键**）

| 连接 | 身份 | 用途 |
|---|---|---|
| `base` | 超级用户 + **表 owner** | 造数、篡改、查 `pg_policies` / `pg_tables`、跑迁移 |
| `base_platform_app` | **NOLOGIN** 组角色 | **不能连接**（只持权限） |
| `base_app` | LOGIN，`base_platform_app` 成员 | **应用连接**（RLS 生效）—— 验隔离**必须**用它 |

> ⚠️ **RLS 对超级用户与 owner 不生效** → 用 `base` 验隔离 = **假绿**（AC-S-155 的
> 「以 `base` 查全部行」那一步**只**用来做**阳性对照**，不是判据本身）。
> ⚠️ `base_app` **不是** superuser、**不带** BYPASSRLS（QA 已实测：`rolsuper=f`、`rolbypassrls=f`）。

### 5.5 关键命令与环境

```bash
export XDG_RUNTIME_DIR=/run/user/$(id -u)
export CARGO_TARGET_DIR=/home/myg133/Projects/system_seed/code/target
export DATABASE_URL_TEST="postgres://base:base@127.0.0.1:5432/base_test"

# 本机无 psql 客户端
podman exec base-postgres psql -U base     -d base_test -tAc "<SQL>"   # 管理员（造数 / 查 pg_catalog）
podman exec base-postgres psql -U base_app -d base_test -tAc "<SQL>"   # 应用角色（RLS 生效）
```

### 5.6 摘要口径（**全需求统一**）

| 项 | 口径 |
|---|---|
| 算法 | SHA-256（原始 32 字节） |
| 编码 | **base64url、无填充**（43 字符，字符集 `^[A-Za-z0-9_-]{43}$`）—— **不是 hex** |
| 参照实现 | `crates/platform-iam/src/crypto.rs:56-60` 的 `digest_base64url`（**测试侧独立实现，不调用生产代码**） |
| 适用列 | `client_secret_hash` / `code_hash` / `device_code_hash` / `user_code_hash` |

> 〔S-16〕仓里**同时**有 `digest_base64url` 与 `digest_hex`（`crypto.rs:62-68`）→
> AC-S-139 / AC-S-142 原文只写「SHA-256 摘要」→ **口径必须一次钉死**。

---

## 6. 挑刺编号反查表（**S-01 ~ S-39 全部命中**）

| 挑刺 | 档位 / 级别 | 受影响用例 | 说明 |
|---|---|---|---|
| **S-01** | 物理不可达 / 阻塞 | TC-022-026 | 第三方回调不可达 → 必须 `page.route` 拦截 |
| **S-02** | 物理不可达 / 阻塞 | TC-022-024 | 未登录回跳丢 query → 整个授权码流程悬空 |
| **S-03** | 不可判定 / 阻塞 | TC-022-111 | `TITLES` 行数「本需求新增数」→ 必须写死 94 或 102 |
| **S-04** | 口径不一致 / 阻塞 | TC-022-113 | `oauth.*` 事件码进 `TITLES` → 既有跨栈测试必红 |
| **S-05** | 不可判定 / 阻塞 | TC-022-103、TC-022-131 | 「二者都接受」不可判定 → 钉死 `UNAUTHENTICATED` |
| **S-06** | 物理不可达 / 阻塞 | TC-022-009 | 「持 `oauth:client:write` 的管理员」不存在 |
| **S-07** | 物理不可达 / 阻塞 | TC-022-091 | mock 接收端在 E2E 起不来（跨进程 + 端口动态） |
| **S-08** | 物理不可达 / 阻塞 | TC-022-079、TC-022-080 | 篡改 ticket 需私钥 → 判据恒真 / 前置不可达 |
| **S-09** | 不可判定 / 阻塞 | TC-022-030、TC-022-031 | 失败点（preview 还是 confirm）未钉 |
| **S-10** | 不可判定 / 阻塞 | TC-022-106、TC-022-107 | 步骤 3 测的是「无 client 认证」不是「令牌被拒」 |
| **S-11** | 口径不一致 / 高 | TC-022-003 | `{issuer}/oauth/authorize` vs `oauth.frontendBaseUrl` |
| **S-12** | 口径不一致 / 高 | TC-022-003 | `{平台}` vs `{issuer}` 两种基准 |
| **S-13** | 口径不一致 / 高 | TC-022-116、TC-022-117、TC-022-118 | 判据命令基线已红 + **不做 C-5** + 文件数不变 |
| **S-14** | 口径不一致 / 高 | TC-022-042、TC-022-043 | 「二者必须能区分开」做不到 → 补**真判据** |
| **S-15** | 不可判定 / 高 | TC-022-044 | 「不回溯扩大」断言的对象不存在 → 改成「先签发再授权」 |
| **S-16** | 口径不一致 / 高 | TC-022-035、TC-022-036、TC-022-061 | 摘要编码未钉 → 全用 base64url 无填充 |
| **S-17** | 不可执行 / 高 | TC-022-021、TC-022-022 | 敏感值扫描无宿主 / 无口径 / 无阳性对照 |
| **S-18** | 不可判定 / 高 | TC-022-067、TC-022-068、TC-022-069、TC-022-074 | 两条都是 401 → 无区分力；错误体形状缺失 |
| **S-19** | 口径不一致 / 高 | TC-022-126 | 「`sub` 齐全」与 D198 / AC-S-141 冲突 |
| **S-20** | 口径不一致 / 高 | TC-022-099、TC-022-100 | frontChannel 数组 vs HTML iframe；安全项无判据 |
| **S-21** | 不可判定 / 高 | TC-022-056、TC-022-057、TC-022-062 | `verification_uri` 未钉；`user_code` 格式 / `slow_down` 未覆盖 |
| **S-22** | 口径不一致 / 高 | TC-022-123 | 4 张表无 manifest `tenantScoped` → 平台 RLS 自检不覆盖 |
| **S-23** | 口径不一致 / 高 | TC-022-124 | RLS 只验读侧 → 补 `WITH CHECK` 写侧 |
| **S-24** | 不可判定 / 高 | TC-022-097 | `sid` 多会话未定义 |
| **S-25** | 安全缺口 / 高 | TC-022-102 | 全局登出不杀已签发的登录 access token |
| **S-26** | 口径不一致 / 中 | TC-022-098 | 「都被撤销」是全局登出的必然后果 → 无区分力 |
| **S-27** | 口径不一致 / 高 | TC-022-012、TC-022-013 | disabled 的拒绝点与错误码未钉（平台话 vs 协议话） |
| **S-28** | 口径不一致 / 高 | TC-022-109 | `OAUTH_CLIENT_SECRET_INVALID` 是死码 |
| **S-29** | 表述 / 中 | TC-022-005 | 「恰好包含」自造词 |
| **S-30** | 不可判定 / 中 | TC-022-033 | `error_description` 措辞不可机械判定 |
| **S-31** | 不可判定 / 中 | TC-022-037 | TTL 补充判据缺列清单 → 判据失真 |
| **S-32** | 表述 / 中 | TC-022-036 | 「查授权码表的行数」口径不清 |
| **S-33** | 口径不一致 / 中 | TC-022-076 | grant 级撤销 → 所有令牌失效，未覆盖 |
| **S-34** | 物理不可达 / 中 | TC-022-087 | 「另一个租户的用户」没人建 |
| **S-35** | 不可判定 / 中 | TC-022-120 | 敏感值扫描的宿主与阳性对照注入点未指定 |
| **S-36** | 口径不一致 / 中 | TC-022-114 | 迁移文件名两套（`…0500_` vs `…0600_`） |
| **S-37** | 口径不一致 / 高 | TC-022-050 | `client_credentials` 的 scope 是几重交集未定义 |
| **S-38** | 口径不一致 / 中 | TC-022-009 | 「201（或 200）」→ 钉 201 |
| **S-39** | 口径不一致 / 高 | TC-022-089 | `consent_required` 的端点与错误体形状未钉 |

---

## 7. 用例对 AC 的「可执行化修正」清单

| 用例 | AC 原文不可执行处 | 修正为 | 挑刺编号 |
|---|---|---|---|
| TC-022-003 | `authorization_endpoint` 钉成 `{issuer}/oauth/authorize` | 改为「path == `/oauth/authorize`，origin == 配置 `oauth.frontendBaseUrl`（未配置时回退 issuer）」 | S-11、S-12 |
| TC-022-005 | 「**恰好包含**」 | 改为「集合**恰好等于**那 3 个」 | S-29 |
| TC-022-009 | 「**201**（或 200）」 | 改为「**201**」（平台创建端点统一口径） | S-38 |
| TC-022-009 | 前置「管理员账号持有 `oauth:client:write`」 | 改为「由 `scripts/seed-e2e.sh` 播种 `u-perm-oauth@example.com` 并**自证**权限已授」 | S-06 |
| TC-022-012 / 013 | 「走一次授权码交换 → 401 `invalid_client`」 | 拆成**两条**：preview → **403 + `OAUTH_CLIENT_DISABLED`**（平台话）；token → **401 + `invalid_client`**（协议话） | S-27 |
| TC-022-021 / 022 | 「全仓扫描 `client_secret` 的明文」 | 拆成「运行期**真值**断言（明文子串）」+「静态扫描（扫描器**可单独调用**）+ 阳性对照」 | S-17 |
| TC-022-024 | 「登录（`tenant-a` / 账号 / 密码）」 | 补「**query 完整保留**（`return_to`）」的断言 —— 现状**做不到** | S-02 |
| TC-022-026 | 「读浏览器最终地址栏的 `code`」 | 补「**先 `page.route` 拦截** `https://client.example/**`」 | S-01 |
| TC-022-030 / 031 | 「**签发 code 这一步**就失败」 | 改为「**preview** 就 400 + 各自平台码 + 不出 `ticket`」 | S-09 |
| TC-022-032 | AC 未测「缺 `code_challenge_method`」 | **新增**一条 → 400 `OAUTH_UNSUPPORTED_CODE_CHALLENGE_METHOD` | S-09 |
| TC-022-033 | 「`error_description` 不含…以外的信息」 | 改为「不含 `code_verifier`/`code_challenge`/`client_secret` 明文」+「不含任何 CJK」 | S-30 |
| TC-022-035 / 061 | 「`SHA-256(code)` 摘要」（未说编码） | 改为「**base64url 无填充**（43 字符，字符集 `^[A-Za-z0-9_-]{43}$`）」 | S-16 |
| TC-022-036 | 「查授权码表的**行数**」 | 改为「按**摘要**查该 code 的行数 **== 0**」+「表里无 `consumed_at` 列」 | S-32 |
| TC-022-037 | 「造一个 `expires_at` 在 90 秒前的授权码」 | 改为「**逐列写死**全部绑定列；两次只差 `expires_at`」 | S-31 |
| TC-022-042 | 「二者必须能区分开（用路径 1 证明）」 | 改为「两条**前提**断言（`platform_permission` 里无 `rbac:role:*`；用户有效集无通配）」 | S-14 |
| TC-022-043 | AC **没有**这条 | **新增**：把 `rbac:role:*` **登记进注册表** → 仍 400（这才是能区分两个原因的判据） | S-14 |
| TC-022-044 | 「路径 2 的令牌不含 write」（路径 2 是 400，**没有令牌**） | 改为「**先成功签发 → 再授权 → 旧令牌不变**；另发新令牌拿到新权限」 | S-15 |
| TC-022-050 | AC **没有**这条 | **新增**：`client_credentials` 请求超出白名单 → 400 `invalid_scope`（**两重**交集） | S-37 |
| TC-022-056 | AC **没测** `user_code` 格式 | **新增**：长度 == 8、逐字符 ∈ base20 | S-21 |
| TC-022-057 | 「浏览器访问 `verification_uri`」（未钉指向） | 改为「path == `/oauth/device`、origin == 授权页基址；`verification_uri_complete` == 基址 + `?user_code=`」 | S-21 |
| TC-022-062 | AC **没测** `slow_down`、**没覆盖** `poll_interval_seconds` / `last_polled_at` | **新增**：轮询过快 → 400 `slow_down`；间隔 +5 秒；`last_polled_at` 非空 | S-21 |
| TC-022-067 / 068 | 「**401**」（无错误体形状） | 改为「**逐字等于** `{"error":"invalid_client"}`（只含 `error` 键，不含 `OAUTH_*`）」 | S-18 |
| TC-022-069 | AC **没有**这条 | **新增**：public client 带 `client_secret_basic` → 同样 401 `invalid_client`（**唯一**能区分「public 无权」与「没带认证」的形态） | S-18 |
| TC-022-074 | AC **没测**撤销端点的未认证 | **新增**：401 `{"error":"invalid_client"}` | S-18 |
| TC-022-076 | AC 只验**一枚**令牌 | 改为「**签发 2 枚** → 撤 grant → **两枚**都 `active=false`」 | S-33 |
| TC-022-080 | AC 的「篡改 ticket」判据**恒真** | **新增**：用**自签**（scope 更大）的票据 → 400 `OAUTH_TICKET_INVALID`（真判据） | S-08 |
| TC-022-087 | 「另一个租户的用户」（没人建） | 补「`tenant-b` 的账号由 `seed-e2e.sh` 播种」+ **阳性对照** | S-34 |
| TC-022-089 | 「返回 `consent_required`」（端点与形状未钉） | 改为「preview 返回 200 + `needsConsent=true`；`consent_required` 只出现在**授权页 query**」 | S-39 |
| TC-022-097 | `sid` 多会话未定义 | 改为「`sid` **逐字等于**触发登出的那次会话 id」 | S-24 |
| TC-022-099 / 100 | 「响应体里 `frontChannel` 数组」（与 design 的 HTML 矛盾）；安全项无判据 | 按 BA 拍板的形态写；**新增** `sandbox` / `X-Frame-Options` 断言 | S-20 |
| TC-022-102 | AC **没断言**登出后旧登录令牌是否失效 | **新增**：BA 二选一（立即失效 / 显式写明现状） | S-25 |
| TC-022-103 / 104 | 「401（`UNAUTHENTICATED` **或** `invalid_token`——二者都接受）」 | 改为「**逐字等于** `UNAUTHENTICATED`」，删掉「二者都接受」 | S-05 |
| TC-022-106 / 107 | 步骤 3 测的是「没带 client 认证」 | 步骤 3 改成「同端点再打一次」；**新增** 107（带合法 client 认证 → 200 `active=true`） | S-10 |
| TC-022-105 | 「**成功**」（对照账号没有 `audit:events:read` → 403） | 改为「**不是 401**」 | — |
| TC-022-108 | AC 只覆盖两个**读**端点 | **新增**：写类端点（`/api/iam/refresh`、`/api/iam/login`）也必须拒 | — |
| TC-022-111 | 「基线 58 + 21 + 5 + **本需求新增数**」 | 改为「**逐项写死**」（94 或 102，取决于 BA 对 S-04 的选择） | S-03 |
| TC-022-113 | AC 只说「人工核对有一行」 | 改为「有**且只有**一行，且来源标签**不是** `platform-org::notify`」 | S-04 |
| TC-022-114 | AC 未提迁移文件名 | 补「种子迁移的**文件名**必须存在（design §8 与 §13 矛盾，S-36）」 | S-36 |
| TC-022-116 | 「跑 `cargo xtask capabilities --check` → 通过（14 个文件与源码一致）」 | 拆成「先还 REQ-014 的 `catalog.rs` 债」+「文件数 **== 14**」+「`permissions.json` 的 `total == 32`」 | S-13 |
| TC-022-117 | 「再跑 C-5」（无命令） | 改为「`cargo test -p platform-oauth --test contract_c5`，且 **N >= 1 passed**（不是 `0 passed` 的假绿）」 | S-13 |
| TC-022-120 | 「对全表做敏感值扫描，**带阳性对照**」（无宿主 / 无键集合） | 改为「扫描器**可单独调用** + 扫描面 = **全部文本列** + 阳性对照行 + **10 个敏感键**逐字比对」 | S-35 |
| TC-022-121 | 「返回 `ok = true`」 | 补「`checked > 0`」（`checked == 0` 的 `ok=true` 是**假绿**） | — |
| TC-022-123 | 「以 `base_app` 查 → 0 行」 | 补「`rolsuper=false`、`rolbypassrls=false`」+「4 张表各 1 条 `<table>_tenant_isolation` policy」+「直接查 `pg_policies`」（不依赖平台自检） | S-22 |
| TC-022-124 | AC 只验读侧；「补充」段容易被误读 | 补「写侧 `WITH CHECK`」+「`platform_oauth_client` **不启用** RLS」写成**主步骤**判据 | S-23 |
| TC-022-126 | 「`sub` … **齐全**」（前置没说令牌种类） | 前置固定为「`authorization_code` 签发的令牌」；`client_credentials` 的形态见 TC-022-049 | S-19 |
| TC-022-129 / 130 | AC 只要求「不得把签名无效与已过期混为一谈」 | **新增** 130（`alg=none` / 算法混淆被拒） | — |

---

## 8. 依赖 BA 先钉死的前提（否则对应用例只能写成「守卫」）

**这 12 项不钉死，对应用例判不出红绿。**（与挑刺报告 §7 一一对应）

| # | 待钉死项 | 阻塞在 | 影响用例 | 建议取值 |
|---|---|---|---|---|
| P1 | **未登录回跳怎么保住 query** | S-02 | 023 ~ 034、088 ~ 090 | 公开路由 + 登录页 `return_to`（不动 REQ-011 既有行为） |
| P2 | **授权 URL 的基址符号** | S-11、S-12 | 003、023、026、088、089 | `{授权页基址}`；未配置 `oauth.frontendBaseUrl` 时回退 `{issuer}` |
| P3 | **8 个 `oauth.*` 事件码进不进 `TITLES`** | S-03、S-04 | 109 ~ 113 | 二选一 → `TITLES` = **94** 或 **102**（数字写死） |
| P4 | **AC-S-151 的 401 口径** | S-05、S-10 | 103 ~ 108 | 钉 `UNAUTHENTICATED`；删「二者都接受」 |
| P5 | **front-channel 是 HTML 还是 JSON** | S-20 | 098 ~ 101 | 选 **HTML + iframe**（design §4.5），并把 `sandbox` / `X-Frame-Options` 写进 AC |
| P6 | **`OAUTH_CLIENT_SECRET_INVALID` 删还是补触发点** | S-28 | 109 ~ 113 | **删**（→ 9 个平台码）；若留则补「轮换 secret」端点 |
| P7 | **4 张表的 RLS policy 谁生成** | S-22、S-23 | 122 ~ 124 | manifest 加 `tenantScoped` 段 → 由 `xtask tenant-policies` 生成 |
| P8 | **摘要的编码口径** | S-16 | 017、035、036、061 | 全用 `digest_base64url`（base64url 无填充） |
| P9 | **全局登出要不要作废已签发的登录 access token** | S-25 | 102 | 至少在 AC 里**显式写明现状**（D211 + 元数据同步） |
| P10 | **`sid` 在多会话下的取值** | S-24 | 097 | 取**触发登出的这次请求**的会话 id |
| P11 | **`client_credentials` 的 scope 是几重交集** | S-37 | 050 | **两重**（请求 ∩ `client.allowed_scopes`），无用户维 |
| P12 | **迁移文件名** | S-36 | 114 | schema `…0500_` / 权限点种子 `…0600_` / 配置种子 `…0700_` |

**另有一项需要**用户**裁定（BA 登记在 decisions.md「待用户裁定」表，本轮不重复提）**：无。

---

## 9. 执行层次与命令（Dev 待补）

| 层 | 内容 | 命令 |
|---|---|---|
| `rust-unit` | scope 交集、PKCE 校验、client 认证、摘要口径 | `cargo test -p platform-oauth --all-targets -- --test-threads=1` |
| `rust-integration`（真库） | client / 授权码 / 设备码 / 令牌 / 授权记录 / RLS | 同上（需 `DATABASE_URL_TEST`） |
| `rust-integration`（权限） | `require_permission` 调用点、C-5 | `cargo test -p platform-oauth --test contract_c5 -- --test-threads=1` |
| `rust-crossstack` | 错误码 / `TITLES` / i18n | `cargo test -p platform-server --test i18n_titles -- --test-threads=1` |
| `rust-regression` | `platform-iam` / `platform-rbac` / `platform-audit` 既有套件全绿 | `cargo test -p platform-iam --all-targets -- --test-threads=1`（再跑 `-p platform-rbac`、`-p platform-audit`） |
| `ops-index` | `capabilities/` 索引刷新 | `cargo xtask capabilities`（**先还 REQ-014 的 `catalog.rs` 债**） |
| `ops-policies` | RLS policy 生成 / 校验 | `cargo xtask tenant-policies` / `--check` |
| `frontend-unit` | 同意页 / 设备码页 / 我的授权页渲染 | `pnpm --filter @base/shell test` |
| `e2e` | 授权端到端 / 第三方回调拦截 / 登出投递 | `pnpm playwright test e2e/oauth.spec.ts` |
| `ops-script` | mock 接收端（超时 / 500 两种失败形态） | `MOCK_MODE=timeout ./scripts/mock-logout-receiver.sh` |

> **低配机约束**（dev-guide §2 / §9.8）：`cargo test` **逐 crate** 跑，`--workspace` 必失败；
> 必须带 `--all-targets`（否则 lib 测试一个都不跑、退出码仍 0 = **假绿**）；
> 只能有**一个** `--`；真实库用例 `-- --test-threads=1`。
> **本轮未跑任何 cargo 命令**（QA 只做只读核实 + `base_test` 上的只读实测）。
