# 验证报告 —— 修复复核（Recheck）

- 需求: REQ-005 交付基线与发布
- 分支: feature/REQ-005 @ `f8b574cc8b2e9443decccc8c29afae0b317c0b3e`
- 首轮报告: feature-meta/qa-report.md（结论 PASS-WITH-RISK，基线 `266dfa9`）
- 复核范围: 5 个改动点 + 恢复 Dev 自述的 3 个漏 + 一次回归（不重做全量验证）
- 验证者: QA Agent（独立复现；自起后端、自停 PG、自计时，未采信 Dev 报告中的任何数字）
- 工具链: rustc/cargo 1.98.1 / podman 6.2.0 + podman-compose 1.6.0 / helm v3.16.3（临时下载到 /tmp 校验，已删）/ pnpm 12.19.0 / bun 1.4.4 / python3 3.14
- 日期: 2026-11-18
- 结论: **PASS-WITH-RISK**

> 核心结论：首轮登记的 **1 中 + 4 低，本轮全部复现为已修复**，5 项逐条自测通过。
> 四条 `detail` 路径我**自起后端实测**，全部为纯 ASCII 英文（`title` 保留中文短句符合契约 C6）；
> PG 停库后 ready 返回 503 的**实测耗时 2.002s**（首轮为 5.003s），显著小于 5s 探针超时；
> prod compose 缺口令/空口令**均非 0 退出**，给有效口令后渲染出的是 `docker.io/library/...` 官方地址。
>
> 但**新发现 2 个低 severity 问题**（均不违反 AC，但都与「首轮修复的正确性」直接相关）：
> ① `BASE_LOG_LEVEL` **完全无效** —— 默认配置下后端**一行日志都不输出**，RUNBOOK §5.1
> 「中文运维说明在日志里」这句**不成立**（即中 1 修复所依赖的兜底通道是死的）；
> ② RUNBOOK §3.1 关于 `podman-compose` 解析 `env_file` 基准的说明**说反了**，且它给出的
> 备选写法 `ENV_FILE=deploy/.env` **实测直接报错**。

---

## 逐项复核

| # | 问题 | Dev 声称 | 复核结果 | 证据 |
|---|------|---------|---------|------|
| **中 1** | 健康检查 4 条路径的 `detail` 全英文、无中文字符（违反契约 C6） | 三个错误路径 `detail` 全部为英文，统一过 `dev_detail()`，5 条单测守着 | ✅ **复现为真（自起后端实测四条路径 + 1 个变体）** | 5 份真实响应体见下表；程序化断言「`detail` 无非 ASCII、无 CJK」→ **全部 detail 全 ASCII：True**。实现：`crates/platform-server/src/health.rs:113-149`（4 个 `detail_*` + `dev_detail()` 只保留可打印 ASCII）；单测 5 条（`health.rs:234-292`）。`title` 为「服务未就绪」（中文短句）—— 契约 C6 只约束 `detail`，**不判为缺陷** |
| **低 2** | 探针超时与连接池超时撞车（均 5s），探针拿不到 503 | `acquire_timeout` 降到 **2s**，探针保持 5s，并加单测守住 | ✅ **复现为真** | `crates/platform-server/src/db.rs:27` → `pub const ACQUIRE_TIMEOUT: Duration = Duration::from_secs(2)`；`db.rs:70-76` 单测 `acquire_timeout_is_shorter_than_the_probe_timeout`（**存在且通过**）；`deploy/helm/base-platform/templates/deployment.yaml:89` → `timeoutSeconds: 5`；`Dockerfile:94` → `HEALTHCHECK --timeout=5s`。**实测**：停 PG 后 ready → `HTTP=503 time=2.002212` / `2.002235` / `2.001611`（连测 3 次）→ **2.002s < 5s** ✓ |
| **低 1** | RUNBOOK/报告称 Helm 默认渲染含 Ingress | RUNBOOK §4 改为「默认 5 类不含 Ingress」，并给 `--set ingress.enabled=true`；上一轮的「六类资源」表述一并更正 | ✅ **复现为真** | `helm template` 默认 → **5 类**（ServiceAccount/ConfigMap/Service/Deployment/Job，`count=5`，**无 Ingress**）；`--set ingress.enabled=true` → **6 类**（多 Ingress，`count=6`）；`helm lint` → `1 chart(s) linted, 0 chart(s) failed`。三处表述均已改正：`deploy/RUNBOOK.md:222-228`、`feature-meta/verification-report.md:27,229-241`、`feature-meta/traceability.md:29`。全仓残留「六类」仅出现在首轮报告与 Dev 报告里**明确指出旧说法不实**的段落 → 无残留错误表述 |
| **低 3** | `core-change-detect.sh --self-test` 未覆盖「平台 crate 必须登记」 | self-test 已并入 `check_core_coverage`，同时覆盖「未登记必须检出」与「已登记必须放行」 | ✅ **复现为真（并做了变异测试证明不是假覆盖）** | `./scripts/core-change-detect.sh --self-test` → `✓ 自检通过：merge-base 口径不误算 base 侧改动，PR 侧核心改动能检出，未登记平台 crate 能检出`，**EXIT=0**。**变异测试（在 /tmp 副本上做，未改交付物）**：把 `check_core_coverage` 的 `return "$missing"` 改成 `return 0` → 自检 **失败**「未登记的平台 crate 没被检出」EXIT=1；改成 `return 1` → 自检 **失败**「已登记的 crate 被误报」EXIT=1 → **两个方向都被真正守住** |
| **低 4** | prod compose 默认弱口令 `base` + 第三方镜像源 | 口令默认值改空（`${POSTGRES_PASSWORD:?…}`），镜像改官方地址 | ✅ **复现为真** | ① 不设口令 → `podman-compose -f deploy/compose.prod.yaml config` **EXIT=1**，报 `required variable POSTGRES_PASSWORD is missing a value: POSTGRES_PASSWORD is required - inject it via .env or a Docker secret`（**未静默用弱口令**）；② `.env` 里 `POSTGRES_PASSWORD=` 空值 → **EXIT=1**，同一报错；③ 有效口令 → **EXIT=0**，渲染出 `docker.io/library/postgres:17-alpine` / `docker.io/library/redis:7-alpine`，grep `daocloud\|docker.m.` → **无命中**（官方地址）；④ 开发用 `compose.yaml:6,49` **仍保留** `docker.m.daocloud.io/library/...` 加速前缀（属开发环境优化，符合预期） |
| **额外 1** | 恢复 Dev：① `prod-preflight.sh` 路径不存在 | 脚本已落到 `scripts/prod-preflight.sh` | ✅ **复现为真** | `ls -la scripts/prod-preflight.sh` → `-rwxr-xr-x`（**存在且可执行**）；按 RUNBOOK §3.1 的 `cd deploy && ../scripts/prod-preflight.sh` **实跑** → `✓ 生产部署前置校验通过…`，EXIT=0。另做 6 组反向测试（缺口令 / 弱口令 `base` / 口令过短 / DATABASE_URL 内弱口令 / 库名 `base_dev`≠`base` / 全部合规）→ **前 5 组均 EXIT=1 并打印修复提示，第 6 组 EXIT=0** |
| **额外 2** | 恢复 Dev：② CI 私有化 compose 校验 job 会挂（空口令） | CI step 级 `env` 注入非空占位口令 | ✅ **复现为真** | `.github/workflows/ci.yml:154-160`：`env: POSTGRES_PASSWORD: ci-validation-only-not-a-real-secret`（**非空**），紧跟在 `cp .env.example deploy/.env`（`:153`）之后。**本地等价复现**：`.env` 里空口令 + shell 里非空占位 → `podman-compose … config` **EXIT=0**，且渲染出 `POSTGRES_PASSWORD: ci-validation-only-not-a-real-secret`（**shell env 覆盖 .env 空值**）→ job 不会挂 |
| **额外 3** | 恢复 Dev：③ compose 头部用法示例把 `.env` 放错目录 | 头部改为「在 deploy/ 目录里执行；.env 与本文件同目录」 | ⚠️ **示例已修对，但 RUNBOOK 的配套说明写反了（见问题 #2）** | 头部（`deploy/compose.prod.yaml:15-21`）现在写 `cp ../.env.example .env` → `.env` 与 compose 文件同目录 ✓。但 RUNBOOK §3.1（`deploy/RUNBOOK.md:115-118`）称「`podman-compose` 按**当前目录**解析服务级 `env_file`」—— **与本机 podman-compose 1.6.0 实测相反**（见问题 #2） |

---

## 四条 detail 路径实测

自起后端：`./target/debug/base serve`（**重新编译过**，`target/debug/base` 原始产物 09:59，早于修复提交 10:13，为避免验到旧代码我先 `cargo build -p platform-server --bin base`）。
真实响应体逐条摘录（`detail` 原样，未截断）：

| 路径 | HTTP | 耗时 | `code` | `detail` 原文 | detail 是否全英文 |
|------|------|------|--------|--------------|-----------------|
| ① PG 正常 → `/health/ready` | **200** | 0.169140s | —（无错误体） | `{"appliedMigrations":[20261118000100],"environment":"dev","status":"ready","traceId":"dc23a784-…","version":"0.1.0"}` | n/a（成功体无 detail） |
| ② **停掉 PG** → `/health/ready` | **503** | **2.002212s**（复测 2.002235 / 2.001611） | `DATABASE_UNAVAILABLE` | `database is unreachable: pool timed out while waiting for an open connection` | ✅ **全 ASCII** |
| ②′ 停掉 PG → `/health/live` | **200** | 0.000686s | — | `{"status":"alive","traceId":"486f5126-…","version":"0.1.0"}` | n/a |
| ③ `MIGRATIONS_PENDING`（临时库 up→down，账本清空） | **503** | 0.158066s | `MIGRATIONS_PENDING` | `pending migrations: [20261118000100]` | ✅ **全 ASCII** |
| ④ `MIGRATION_DIRTY`（`_sqlx_migrations.success=false`） | **503** | 0.001999s | `MIGRATION_DIRTY` | `migration 20261118000100 is in a dirty state and needs manual recovery` | ✅ **全 ASCII** |
| ⑤ 变体：迁移表读不出来（`DROP TABLE _sqlx_migrations`） | **503** | 0.002497s | `MIGRATIONS_PENDING` | `migration state is unreadable, migrations may not have run: error returned from database: relation "_sqlx_migrations" does not exist` | ✅ **全 ASCII** |

- 原始响应（②）：`{"type":"https://errors.base.example/service-not-ready","title":"服务未就绪","status":503,"detail":"database is unreachable: pool timed out while waiting for an open connection","instance":"/health/ready","code":"DATABASE_UNAVAILABLE","trace_id":"f1850ebf-…"}`
- 原始响应（③）：`{"type":"https://errors.base.example/service-not-ready","title":"服务未就绪","status":503,"detail":"pending migrations: [20261118000100]","instance":"/health/ready","code":"MIGRATIONS_PENDING","trace_id":"70cf1fbe-…"}`
- 原始响应（④）：`{"type":"https://errors.base.example/service-not-ready","title":"服务未就绪","status":503,"detail":"migration 20261118000100 is in a dirty state and needs manual recovery","instance":"/health/ready","code":"MIGRATION_DIRTY","trace_id":"3a94c932-…"}`
- 原始响应（⑤）：`{"type":"https://errors.base.example/service-not-ready","title":"服务未就绪","status":503,"detail":"migration state is unreadable, migrations may not have run: error returned from database: relation \"_sqlx_migrations\" does not exist","instance":"/health/ready","code":"MIGRATIONS_PENDING","trace_id":"1097d1a7-…"}`
- **程序化复核**（python3 逐字符判定 `c.isascii()` 与 CJK 码位区间）→ 五条 `detail` **全部无非 ASCII、无 CJK**：「全部 detail 无中文字符： True」
- `title` 恒为「服务未就绪」（中文短句）—— 契约 C6 只约束 `detail`，**符合契约，不判为缺陷**。
- PG 恢复后 `/health/ready` → **HTTP 200** `{"appliedMigrations":[20261118000100],"environment":"dev","status":"ready",…}`（0.181925s）。

---

## 回归

- **Rust 138/138**（`cargo test --workspace -- --test-threads=2` → `passed=138 failed=0`；分布：platform-core 41 + platform-contracts 8 + platform-server(lib) 32 + base(bin) 5 + sandbox 2 + contracts 4 + xtask 46；3 个 doctest ignored）
  - **首轮 132 个必须仍全绿 ✓**：132 → 138，增量 **+6 全部来自新增单测**（`health.rs` 3→8 条：`readiness_detail_is_english_on_every_error_path` / `dirty_migration_error_carries_english_detail` / `pending_migrations_error_carries_english_detail` / `healthy_database_has_no_error_and_no_detail` / `dev_detail_strips_non_ascii_from_database_errors`；`db.rs` 2→3 条：`acquire_timeout_is_shorter_than_the_probe_timeout`）；**逐个比对首轮 7 个测试函数名，无一被删除**
- **前端 23/23**（`pnpm -r test`：i18n 3 + auth 3 + schema 3 + hooks 3 + sdk 2 + theme 2 + ui 3 + shell 4，8 个 workspace 包全绿）
- **E2E 1/1**（`bun run test:e2e` → `✓ 1 [chromium] › e2e/shell.spec.ts:3:1 › 底座壳渲染并可过滤菜单 (268ms)`，`1 passed (4.9s)`）
- **Lint / 格式**：`cargo clippy --workspace --all-targets -- -D warnings` → **0 告警**（exit 0）；`cargo fmt --all --check` → **通过**（exit 0）
- **静态门禁**：`cargo xtask capabilities --check` → 「能力索引校验通过：13 个文件与源码一致」；`check-deps` → 「平台 crate 未反向依赖业务模块」；`check-modules` → 「模块目录校验通过：1 个模块 ✓ sandbox」—— 全 exit 0
- **Helm**：`helm lint` → 0 failed；`helm template` 默认 5 类 / `--set ingress.enabled=true` 6 类

### 通用清单

| 项 | 结果 | 证据 |
|----|------|------|
| 无 TODO/FIXME/XXX/HACK | ✅ | 本轮改动文件（`.rs/.ts/.tsx/.sh/.yaml/.sql/.md`）grep → **0 处** |
| 无调试输出 | ✅ | 无 `dbg!`；`eprintln!` 仅 3 处，均在 `main.rs:107-115` 的**配置 fail-fast 错误输出**路径（属功能，非调试） |
| `.env` 未入库 | ✅ | `git ls-files | grep .env` → 只有 `.env.example`；`.gitignore:20-22` 排除 `.env` / `.env.*`（放行 `!.env.example`）；磁盘上 `.env` 与 `deploy/.env` **均不存在** |
| `.env.example` 无弱口令默认值 | ✅ | `.env.example:14` → `POSTGRES_PASSWORD=`（**空值**） |
| 无硬编码密钥 | ✅ | 全仓扫描（`password/secret/token/api_key`）→ 唯一命中是 `scripts/prod-preflight.sh:109` 的**变量引用** |
| `git diff` 对 `crates/platform-core/`、`crates/platform-contracts/` 为空 | ✅ | `git diff develop..HEAD -- crates/platform-core crates/platform-contracts crates/platform-contracts-macros --stat` → **输出为空** |

---

## 仍存在的问题

| # | 级别 | 描述 | 证据 | 建议 |
|---|------|------|------|------|
| **1** | **低** | **`BASE_LOG_LEVEL` 完全无效，默认配置下后端一行日志都不输出。** `init_tracing` 的默认过滤器写死 `platform_server={level},tower_http=info`，但 `main.rs` 里的事件属于**二进制 target `base`**（不是 lib target `platform_server`）→ 过滤器一个都匹配不上。**后果**：① `.env.example:34` 承诺的 `BASE_LOG_LEVEL` 开关是摆设；② RUNBOOK §5.1 新写的「运维视角的中文说明在**日志**里（`tracing::warn!` 记 `code`+`detail`+`pending`+`dirty`）」**不成立** —— 中 1 修复把中文运维说明「移到日志」，而**这条兜底通道是死的**，中文运维说明实际**彻底丢失**（既不在响应体，也不在日志） | `crates/platform-server/src/main.rs:177-181`（`EnvFilter::new(format!("platform_server={level},tower_http=info"))`）+ `crates/platform-server/Cargo.toml`（`[lib] name="platform_server"` / `[[bin]] name="base"`，`main.rs` 属 bin → tracing target = `base`）。**实测**：`BASE_LOG_LEVEL=warn|info|debug` 三档 → 日志文件均 **0 字节**（连启动 INFO「底座服务已启动」都没有）；对照组 `RUST_LOG=warn` → **455 字节**，能看到 `WARN base: readiness 未通过… code="MIGRATIONS_PENDING" detail="migration state is unreadable…"` | ① 把过滤器改成同时覆盖 bin target：`EnvFilter::new(format!("base={level},platform_server={level},tower_http=info"))`（或直接用 `format!("warn,platform_server={level},base={level}")`）；② 加一条单测/冒烟断言「默认配置下 `tracing::warn!` 能被采集到」防回潮；③ 若不想现在改代码，**至少把 RUNBOOK §5.1 那句改成如实描述**（当前文档与实际不符，且它正是中 1 修复的正当性依据） |
| **2** | **低** | **RUNBOOK §3.1 关于 `podman-compose` 解析服务级 `env_file` 基准的说明写反了，且它给出的备选写法实测直接报错。** RUNBOOK 称「`docker compose` 按 compose 文件所在目录解析，`podman-compose` 按**当前目录**解析 —— 用 `podman-compose` 时请在 `deploy/` 目录里执行，或显式指定 `ENV_FILE=deploy/.env`」。实测（podman-compose 1.6.0）：服务级 `env_file` **按 compose 文件所在目录**解析，与 `docker compose` **一致**；照 RUNBOOK 的备选写法从仓库根执行 `ENV_FILE=deploy/.env podman-compose -f deploy/compose.prod.yaml …` → **报错 `Env file at <repo>/deploy/deploy/.env does not exist`，exit 1** | `deploy/RUNBOOK.md:115-118`。**实测**（真实 `compose.prod.yaml` + 仓库式目录布局）：S1 从仓库根跑 `-f deploy/compose.prod.yaml`（`.env` 在 `deploy/`）→ **EXIT=0 解析成功**；S2 `ENV_FILE=deploy/.env` 从仓库根跑 → **EXIT=1**，`ValueError: Env file at /tmp/qa_real/deploy/deploy/.env does not exist`；S3 在 `deploy/` 里跑 → **EXIT=0**。旁证：podman-compose 源码 `podman_compose.py:1355` → `os.path.realpath(os.path.join(dirname, path))`，`dirname` = compose 文件目录 | ① 把 §3.1 那句改成「两种 compose 实现**都按 compose 文件所在目录**解析服务级 `env_file`」；② **删掉** `ENV_FILE=deploy/.env` 这个备选写法（它是坏的）；③ 保留「在 `deploy/` 目录里执行」作为推荐用法（**实测该用法可行**，属最稳妥建议）。注：compose 文件**头部**的说明（「compose 从 compose 文件所在目录读 .env」）本身是**对的**，无需改 |
| **3** | **信息** | `podman-compose` 遇到必填变量缺失时抛的是 **Python 原始 traceback**（`ValueError: required variable POSTGRES_PASSWORD is missing a value: …`），不是人话错误提示。退出码与语义都正确（exit 1，不静默用弱口令），只是排障体验差 | `podman-compose -f deploy/compose.prod.yaml config`（不设口令）完整输出见上（10 行 traceback）。同类问题在 `docker compose` 下会是干净的一行 `error: …` | 客户侧以 `docker compose` 为主，影响有限；若客户用 `podman-compose`，可在 RUNBOOK §3.1 补一句「podman-compose 的报错是 Python traceback，看最后一行 `ValueError:` 即可」 |
| **4** | **信息** | `.env.example:21` 的 `DATABASE_URL=postgres://base:base@postgres:5432/base_dev` 仍带 `base:base` 弱口令与 `base_dev` 库名（开发用值），与 prod compose 的 `POSTGRES_DB=base` 不一致 | `.env.example:16,21`。**但安全网在**：`prod-preflight.sh` 对「DATABASE_URL 内弱口令」与「库名不一致」**都实测报 EXIT=1**（见「额外 1」的 6 组反向测试） | 保持现状可接受（preflight 已兜住）；若要更干净，可把 `.env.example` 的 `DATABASE_URL` 留空并在注释里给 prod 模板 |
| **5** | **信息** | **本轮指定的交付物路径 `feature-meta/qa-report-recheck.md` 在 git 里**已存在一份 REQ-004 的复核报告**（BA 归档于 `beee35f`），本轮按任务指定的唯一交付物路径写入 → 工作区里那份被**原地覆盖**。**REQ-004 的内容完整保存在 git 历史里，没有丢失**（`beee35f` / `b0a26c9` / `3ff22fb` 三次提交均可取回） | `git log --oneline -- feature-meta/qa-report-recheck.md` → `beee35f` / `b0a26c9` / `3ff22fb`；`git show HEAD:feature-meta/qa-report-recheck.md` 首行仍是「需求: REQ-004 AI 引导体系与能力索引 … 分支: feature/REQ-004 @ f45ef882…」（23593 字节），本文件为 REQ-005 版（21177 字节） | 与首轮报告**问题 #6** 同源：BA 归档时把上一轮文件**改名保留**（如 `qa-report-recheck-REQ-004.md`），避免历史报告在工作区被无声覆盖。**本 QA 不提交任何改动**，该决定权留给 BA |

---

## 未验证

| 项 | 原因 | 影响 / 由谁补 |
|----|------|--------------|
| **CI 里 `docker compose -f deploy/compose.prod.yaml config --quiet` 这一条命令的实跑** | 本机 docker CLI 27.2.1 **没有 compose 插件**（`docker compose` → `not a docker command`；`/usr/libexec/docker/cli-plugins/` 等目录均无插件，也无独立 `docker-compose`），本轮**未执行该命令原样** | 已用**等价复现**覆盖其两个关键语义：① `${POSTGRES_PASSWORD:?…}` 在缺失/空值时 **exit 1**（podman-compose 实测，源码 `REQUIRED_NONEMPTY` 语义相同）；② **shell env 覆盖 `.env` 空值**（podman-compose 实测 exit 0 且渲染出占位口令）。CI 里注入的是**非空**占位口令（`ci.yml:159`）→ 该 job 判断为不会挂，但**未在真 Docker Compose 上跑过** |
| **GitHub Actions 整体实跑** | 仓库 `git remote -v` 为空，无 remote，Actions 不可能触发 | CI 配置做了逐行静态核对（12 个 job、`pull_request_review` 触发器仍在、build-image 体积校验仍在），**未在真实 GitHub 上跑过** |
| **镜像实构建**（体积 <100MB、容器内启动、容器内 `HEALTHCHECK --timeout=5s` 与 2s acquire_timeout 的关系、非 root） | 本机内存不足，按任务约束**不构建镜像** | `Dockerfile:94` 的 `--timeout=5s` 为静态核对（5s > 2s，关系成立）；容器内真实行为由 CI `build-image` job 兜住 |
| **真实 K8s 集群的探针行为**（`timeoutSeconds: 5` vs 2.002s 实测值） | 本机无 K8s 集群 | 静态关系成立且已有单测守住（`acquire_timeout_is_shorter_than_the_probe_timeout`）；集群内真实行为未实跑 |
| **Trivy 镜像漏洞扫描** | 需联网拉扫描器与漏洞库 | 由 CI `image-scan` job（CRITICAL/HIGH 阻断）验证 |
| **`prod-preflight.sh` 的 `ENV_FILE` 分支（指定非默认 env 文件路径）** | 已确认脚本支持 `ENV_FILE=` 覆盖，但本轮未对**该分支**做独立断言 | 影响面小（默认分支 6 组场景已全测） |

---

## 清理确认

- 自建临时库 `base_qa_recheck`（MIGRATIONS_PENDING / MIGRATION_DIRTY / 迁移表读不出来 三种变体用）→ **已 DROP**；`pg_database` 回到 `postgres / base_dev / base_test`（与开工前一致）
- `base_dev` 未被污染：表清单仍为 `_sqlx_migrations / platform_meta / platform_seed_log`，迁移账本 `20261118000100 | t` 完好
- 我起的全部 `./target/debug/base serve` 进程 → **已 kill**（残留 2 个，已确认无残留）
- `/tmp` 下我的临时物（helm 二进制与缓存、`/tmp/qa_*` 目录、`/tmp/pc_*`、`/tmp/r_*.json`、日志与断言脚本）→ **已全部删除**
- 容器状态：`base-postgres` / `base-redis` 均 **healthy**
- `git status --porcelain` → **仅 `M feature-meta/qa-report.md`**（**首轮报告**的未提交改动，mtime 09:52:32，**早于本轮开工 10:14，非本轮所改**；按要求**未覆盖、未改动**该文件）
- 本轮唯一写入的文件：`feature-meta/qa-report-recheck.md`（本文件）。**注意**：该路径在 git 里原本已有 BA 归档的 **REQ-004 复核报告**（`beee35f`），本轮按任务指定路径写入 → 工作区那份被原地覆盖；**REQ-004 内容完整保存在 git 历史中，未丢失**（详见「仍存在的问题」#5）
- **本 QA 未做任何 git 提交 / 推送**，工作区改动留给 BA 处理

**VERDICT: PASS-WITH-RISK**
