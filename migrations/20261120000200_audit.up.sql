-- 审计三表 + RLS + append-only + 受限的特权函数（REQ-014）。
--
-- # 三张表
--
-- | 表 | 作用 |
-- |----|------|
-- | `platform_audit_log` | 在线审计记录（契约 C12 的 **13 个固定字段**，不增不减） |
-- | `platform_audit_archive` | 归档表（**同样的 13 个字段** + `archived_at`） |
-- | `platform_audit_chain_state` | 链基准（`head_hash`）—— 没有它，**归档一次就「链坏了」**（D159） |
--
-- # 为什么 `before` / `after` 是 TEXT 而不是 JSONB（D141）
--
-- 哈希覆盖的是**存进列里的那段文本本身**。校验方读回该文本直接参与哈希 →
-- **不需要重新实现任何 JSON 序列化规则** → 测试与生产在「哈希公式 + 链连接」上
-- 真的独立。若列是 `JSONB`，「键序 / 数字表示 / Unicode 转义」差一个字节就会让
-- 「链校验失败」与「实现有 bug」无法区分 → 假绿。
--
-- # 写入走 `SECURITY DEFINER` 函数（D132 / D133）
--
-- - 写入：`platform_audit_append` —— 应用角色**不**直接 INSERT（RLS 会挡住跨租户写，
--   而登录失败等场景需要写「请求携带的租户」）；
-- - 归档：`platform_audit_archive_move` —— 应用角色**不能**有 DELETE 权限
--   （有的话 append-only 立刻破功，AC-S-123 / AC-S-131 直接失败）；
-- - 链基准读取：`platform_audit_head_hash`。
--
-- # append-only 靠 REVOKE 实现（D133），**但种子脚本会把它盖回去**（D147）
--
-- `deploy/seed/02-app-role.sql` 的 `GRANT … ON ALL TABLES IN SCHEMA public`
-- 与 `ALTER DEFAULT PRIVILEGES … UPDATE, DELETE` 会在**重跑种子脚本**时把这里的
-- REVOKE 盖回去 → 所以**种子脚本里也必须有对应的例外块**。
-- 只写迁移里的 REVOKE 是不够的 —— 第一次跑是绿的，补建脚本之后才红。
--
-- # 锁法（D161）
--
-- ⚠️ **禁用** `SELECT … ORDER BY created_at DESC LIMIT 1 FOR UPDATE` 锁上一条：
-- READ COMMITTED（sqlx 默认）下它**防不住**链分叉 —— 后到的事务在拿到锁后
-- **仍然看不到**先到事务新插入的那一行 → 两者的 `prev_hash` 相同 → 分叉。
--
-- → 用 `pg_advisory_xact_lock(hashtext('audit:' || tenant_id))` 串行化同租户写入，
-- **拿到锁后用新语句重读**最后一条。

-- ---------------------------------------------------------------------------
-- 1. 在线表（契约 C12 的 13 个固定字段）
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS platform_audit_log (
    id          TEXT        PRIMARY KEY,
    tenant_id   TEXT        NOT NULL,
    actor_id    TEXT,
    action      TEXT        NOT NULL,
    object_type TEXT        NOT NULL,
    object_id   TEXT,
    before      TEXT,
    after       TEXT,
    ip          TEXT        NOT NULL DEFAULT '',
    user_agent  TEXT        NOT NULL DEFAULT '',
    created_at  TIMESTAMPTZ NOT NULL,
    prev_hash   TEXT        NOT NULL,
    hash        TEXT        NOT NULL
);

COMMENT ON TABLE platform_audit_log IS '在线审计记录（契约 C12 的 13 个固定字段；只允许 INSERT —— append-only）';
COMMENT ON COLUMN platform_audit_log.before IS '变更前：**规范化后的 JSON 文本**；无变更时为 NULL（哈希里按零字节空串参与，D142 §3）';
COMMENT ON COLUMN platform_audit_log.after IS '变更后：**规范化后的 JSON 文本**；无变更时为 NULL（哈希里按零字节空串参与，D142 §3）';
COMMENT ON COLUMN platform_audit_log.ip IS '来源 IP（**客户端可控的头**，用于关联与排查，不是取证证据 —— D149）；取不到时为空串';
COMMENT ON COLUMN platform_audit_log.user_agent IS '设备（User-Agent）；取不到时为空串';
COMMENT ON COLUMN platform_audit_log.created_at IS '由写入函数的 clock_timestamp() 生成（同租户严格递增，D143）';
COMMENT ON COLUMN platform_audit_log.prev_hash IS '同租户上一条的 hash；本租户首条为 genesis（64 个 0）';

CREATE INDEX IF NOT EXISTS platform_audit_log_tenant_created_idx
    ON platform_audit_log (tenant_id, created_at DESC);
CREATE INDEX IF NOT EXISTS platform_audit_log_tenant_actor_idx
    ON platform_audit_log (tenant_id, actor_id, created_at DESC);
CREATE INDEX IF NOT EXISTS platform_audit_log_tenant_action_idx
    ON platform_audit_log (tenant_id, action, created_at DESC);
CREATE INDEX IF NOT EXISTS platform_audit_log_tenant_object_idx
    ON platform_audit_log (tenant_id, object_type, object_id, created_at DESC);

-- ---------------------------------------------------------------------------
-- 2. 归档表（同样的 13 个字段 + archived_at）
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS platform_audit_archive (
    id          TEXT        PRIMARY KEY,
    tenant_id   TEXT        NOT NULL,
    actor_id    TEXT,
    action      TEXT        NOT NULL,
    object_type TEXT        NOT NULL,
    object_id   TEXT,
    before      TEXT,
    after       TEXT,
    ip          TEXT        NOT NULL DEFAULT '',
    user_agent  TEXT        NOT NULL DEFAULT '',
    created_at  TIMESTAMPTZ NOT NULL,
    prev_hash   TEXT        NOT NULL,
    hash        TEXT        NOT NULL,
    archived_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

COMMENT ON TABLE platform_audit_archive IS '归档审计记录（与在线表字段相同 + archived_at；同样 append-only）';

CREATE INDEX IF NOT EXISTS platform_audit_archive_tenant_created_idx
    ON platform_audit_archive (tenant_id, created_at DESC);

-- ---------------------------------------------------------------------------
-- 3. 链基准表（D159 + **D171 的 `tip_hash`**）
-- ---------------------------------------------------------------------------
--
-- 🔑 **为什么必须有它**：归档把在线表的一段搬走后，在线表**第一条**的 `prev_hash`
-- 指向的是**已被搬走**的那条 → 接续校验**必然误报断裂** → 「归档一次就链坏了」。
--
-- | 列 | 语义 | 谁更新 |
-- |----|------|--------|
-- | `head_hash` | 在线段的**基准**（genesis 或上次归档的尾 hash） | **只在归档时**更新 |
-- | `tip_hash` | 链**尖端**（最新一条的 `hash`） | **写入时**更新；归档时重算 |
--
-- ⚠️ **只有 `head_hash` 时末尾截断检不出来**（D171）：链内**相邻**断裂仍能检出，
-- 但「尾巴被砍了」查不出来 —— 因为链尖不落库。
-- → 校验时：接续校验用 `head_hash`；**截断检测**用 `tip_hash`
-- （在线表最后一条的 `hash` 必须 == `tip_hash`）。
--
-- 🔑 `platform_audit_chain_state` 是**本需求自建**表 → 加列**不违反契约 C12**
-- （C12 只钉死 `platform_audit_log` 的 13 个字段）。
CREATE TABLE IF NOT EXISTS platform_audit_chain_state (
    tenant_id   TEXT        PRIMARY KEY,
    head_hash   TEXT        NOT NULL,
    tip_hash    TEXT        NOT NULL DEFAULT repeat('0', 64),
    archived_at TIMESTAMPTZ,
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- 🔑 幂等补列：**必须排在 `COMMENT ON COLUMN` 之前** ——
-- 表已存在时（迁移重跑 / 上一版已建过表）`CREATE TABLE IF NOT EXISTS` 是**空操作**
-- → 列还没加 → 随后的 `COMMENT ON COLUMN … tip_hash` 会报「列不存在」而整条迁移失败。
ALTER TABLE platform_audit_chain_state
    ADD COLUMN IF NOT EXISTS tip_hash TEXT NOT NULL DEFAULT repeat('0', 64);

COMMENT ON TABLE platform_audit_chain_state IS '每个租户链的**当前基准**（genesis 或上次归档那段的尾 hash）与**链尖端**（D171：tip_hash 用于末尾截断检测）';
COMMENT ON COLUMN platform_audit_chain_state.head_hash IS '在线段的**基准**（genesis 或上次归档那段的尾 hash）—— 接续校验用它';
COMMENT ON COLUMN platform_audit_chain_state.tip_hash IS '链**尖端**（最新一条的 hash）—— **末尾截断检测**用它（D171）';

-- ---------------------------------------------------------------------------
-- 4. RLS（**手写**，不走 `cargo xtask tenant-policies` 生成器 —— D166）
-- ---------------------------------------------------------------------------
--
-- ⚠️ `current_setting('app.tenant_id', true)` 的第二个参数 `true` **不能省** ——
-- 省了会让「未设租户上下文」的查询直接报错；留着才是 **fail closed**（0 行）。
ALTER TABLE platform_audit_log ENABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS platform_audit_log_tenant_isolation ON platform_audit_log;
CREATE POLICY platform_audit_log_tenant_isolation ON platform_audit_log
    USING (tenant_id = current_setting('app.tenant_id', true))
    WITH CHECK (tenant_id = current_setting('app.tenant_id', true));

ALTER TABLE platform_audit_archive ENABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS platform_audit_archive_tenant_isolation ON platform_audit_archive;
CREATE POLICY platform_audit_archive_tenant_isolation ON platform_audit_archive
    USING (tenant_id = current_setting('app.tenant_id', true))
    WITH CHECK (tenant_id = current_setting('app.tenant_id', true));

ALTER TABLE platform_audit_chain_state ENABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS platform_audit_chain_state_tenant_isolation ON platform_audit_chain_state;
CREATE POLICY platform_audit_chain_state_tenant_isolation ON platform_audit_chain_state
    USING (tenant_id = current_setting('app.tenant_id', true))
    WITH CHECK (tenant_id = current_setting('app.tenant_id', true));

-- ---------------------------------------------------------------------------
-- 5. 应用角色授权（组不存在时跳过 —— 生产角色由部署流程创建）
-- ---------------------------------------------------------------------------
--
-- append-only：只给 SELECT / INSERT，**收回** UPDATE / DELETE（D133）。
DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'base_platform_app') THEN
        EXECUTE 'GRANT SELECT, INSERT ON platform_audit_log TO base_platform_app';
        EXECUTE 'REVOKE UPDATE, DELETE ON platform_audit_log FROM base_platform_app';

        EXECUTE 'GRANT SELECT, INSERT ON platform_audit_archive TO base_platform_app';
        EXECUTE 'REVOKE UPDATE, DELETE ON platform_audit_archive FROM base_platform_app';

        -- 链基准表：应用角色**只读**（更新只发生在两个特权函数里）。
        EXECUTE 'GRANT SELECT ON platform_audit_chain_state TO base_platform_app';
        EXECUTE 'REVOKE INSERT, UPDATE, DELETE ON platform_audit_chain_state FROM base_platform_app';
    END IF;
END
$$;

-- ---------------------------------------------------------------------------
-- 6. 规范哈希函数（D131 / D142）
-- ---------------------------------------------------------------------------
--
-- 拼接规则（**逐字节**钉死，差一个字节哈希就永远对不上）：
--
--     canonical = prev_hash_bytes
--               + for f in [tenant_id, actor_id, action, object_type, object_id,
--                           before, after, ip, user_agent, created_at]:
--                     len(f).to_string() + ":" + f
--     hash      = hex(sha256(canonical))
--
-- - `len(f)` 是**字节长度**（`octet_length`，不是 `length` —— 后者是字符数）；
-- - `prev_hash` 以 **64 字节文本**参与拼接（不是 32 字节二进制）；
-- - 空值用**空字符串**（零字节），不是 `"null"`、不是 `"{}"`。
CREATE OR REPLACE FUNCTION platform_audit_canonical_hash(
    p_prev_hash TEXT,
    p_tenant_id TEXT,
    p_actor_id TEXT,
    p_action TEXT,
    p_object_type TEXT,
    p_object_id TEXT,
    p_before TEXT,
    p_after TEXT,
    p_ip TEXT,
    p_user_agent TEXT,
    p_created_at TEXT
) RETURNS TEXT
LANGUAGE sql
IMMUTABLE
AS $$
    SELECT encode(
        sha256(
            convert_to(COALESCE(p_prev_hash, ''), 'UTF8')
            || (
                SELECT string_agg(
                           convert_to(
                               octet_length(convert_to(f, 'UTF8'))::text || ':' || f,
                               'UTF8'
                           ),
                           '' ORDER BY ord
                       )
                  FROM unnest(
                           ARRAY[
                               COALESCE(p_tenant_id, ''),
                               COALESCE(p_actor_id, ''),
                               COALESCE(p_action, ''),
                               COALESCE(p_object_type, ''),
                               COALESCE(p_object_id, ''),
                               COALESCE(p_before, ''),
                               COALESCE(p_after, ''),
                               COALESCE(p_ip, ''),
                               COALESCE(p_user_agent, ''),
                               COALESCE(p_created_at, '')
                           ]
                       ) WITH ORDINALITY AS t(f, ord)
            )
        ),
        'hex'
    );
$$;

COMMENT ON FUNCTION platform_audit_canonical_hash(TEXT, TEXT, TEXT, TEXT, TEXT, TEXT, TEXT, TEXT, TEXT, TEXT, TEXT) IS '审计链的规范哈希（D131：固定 10 字段 + 长度前缀拼接 + sha256）';

-- ---------------------------------------------------------------------------
-- 7. DB 侧的敏感键扫描器（**权威**，D156 + **D172**）
-- ---------------------------------------------------------------------------
--
-- ⚠️ 这 14 个键与 Rust 侧的 `SENSITIVE_KEYS`
-- （`crates/platform-core/src/extension/audit.rs`）**必须逐项相等** ——
-- `platform-audit` 的一致性测试会从**本文件**抽出下面这段数组比对。
--
-- 改这里必须同步改 Rust 侧，否则一致性测试变红（这是**故意的**）。
--
-- 匹配规则（D172）：键名先经 `platform_audit_normalize_key` 归一化
-- （**驼峰拆词** + 全小写 + `-` 视为 `_`）→ 与基线键**精确相等**
-- （**不是子串**）；**递归**检查每一层（含数组里的对象）。
--
-- 🔑 **归一化两侧必须逐字一致** —— 一致性测试会拿一组样例键
-- 分别跑 Rust 侧与 DB 侧的归一化并比对结果。

-- ---------------------------------------------------------------------------
-- 7.1 键名归一化（D172）—— 与 Rust 侧 `normalize_sensitive_key` **同一套规则**
-- ---------------------------------------------------------------------------
--
-- 规则（**按序**）：
--
-- 1. **缩写边界**：`X Y z`（大写 + 大写 + 小写）→ `X _ Y z`（`APIKey` → `API_Key`）；
-- 2. **驼峰边界**：小写 / 数字与**随后**的大写字母之间插 `_`（`apiKey` → `api_Key`）；
-- 3. 全部转**小写**；
-- 4. `-` 视为 `_`。
--
-- ⚠️ **只认 ASCII**：用 `ascii()` 的码位区间判断，**不**用正则的 `[A-Z]` ——
-- 正则的字符区间**受排序规则影响**（中文 / 土耳其语环境下 `[A-Z]` 会漂移）
-- → 两侧归一化结果不一致 → 一致性测试假红。
--
-- ⚠️ 规则 1 命中后**消费 3 个字符**（`X` `Y` `z`），与 Rust 侧的
--    「非重叠、左到右」扫描**逐字对齐**。
CREATE OR REPLACE FUNCTION platform_audit_normalize_key(p_key TEXT)
RETURNS TEXT
LANGUAGE plpgsql
IMMUTABLE
AS $$
DECLARE
    v_len   INTEGER;
    v_chars TEXT;
    v_out   TEXT := '';
    v_pass1 TEXT;
    i       INTEGER;
BEGIN
    IF p_key IS NULL THEN
        RETURN NULL;
    END IF;

    v_chars := p_key;
    v_len := length(p_key);

    -- 规则 1：缩写边界（`XYz` → `X_Yz`）。
    i := 1;
    WHILE i <= v_len LOOP
        IF i >= 3
           AND i < v_len
           AND ascii(substr(v_chars, i, 1)) BETWEEN 65 AND 90
           AND ascii(substr(v_chars, i - 1, 1)) BETWEEN 65 AND 90
           AND ascii(substr(v_chars, i + 1, 1)) BETWEEN 97 AND 122
        THEN
            v_out := v_out || '_' || substr(v_chars, i, 1) || substr(v_chars, i + 1, 1);
            i := i + 2;
        ELSE
            v_out := v_out || substr(v_chars, i, 1);
            i := i + 1;
        END IF;
    END LOOP;

    -- 规则 2：驼峰边界（小写 / 数字与随后的大写字母之间插 `_`）。
    v_pass1 := v_out;
    v_out := '';
    i := 1;
    WHILE i <= length(v_pass1) LOOP
        IF i >= 2
           AND ascii(substr(v_pass1, i, 1)) BETWEEN 65 AND 90
           AND (
                   ascii(substr(v_pass1, i - 1, 1)) BETWEEN 97 AND 122
                OR ascii(substr(v_pass1, i - 1, 1)) BETWEEN 48 AND 57
               )
        THEN
            v_out := v_out || '_' || substr(v_pass1, i, 1);
        ELSE
            v_out := v_out || substr(v_pass1, i, 1);
        END IF;
        i := i + 1;
    END LOOP;

    -- 规则 3 + 4：转小写；`-` 视为 `_`。
    RETURN replace(lower(v_out), '-', '_');
END;
$$;

COMMENT ON FUNCTION platform_audit_normalize_key(TEXT) IS '敏感键的键名归一化（D172：驼峰拆词 + 全小写 + `-` 视为 `_`）；与 Rust 侧 normalize_sensitive_key 必须逐字一致';

CREATE OR REPLACE FUNCTION platform_audit_sensitive_keys(p_json TEXT)
RETURNS TEXT[]
LANGUAGE plpgsql
IMMUTABLE
AS $$
DECLARE
    -- SENSITIVE-KEY-BASELINE-BEGIN
    v_baseline TEXT[] := ARRAY[
        'password', 'passwd', 'pwd', 'secret', 'client_secret', 'token',
        'access_token', 'refresh_token', 'id_token', 'authorization',
        'credential', 'api_key', 'private_key', 'cookie'
    ];
    -- SENSITIVE-KEY-BASELINE-END
    v_root JSONB;
    v_result TEXT[] := ARRAY[]::TEXT[];
BEGIN
    IF p_json IS NULL THEN
        RETURN v_result;
    END IF;

    BEGIN
        v_root := p_json::jsonb;
    EXCEPTION WHEN others THEN
        -- 不是合法 JSON → 没有可命中的键（不是错误）。
        RETURN v_result;
    END;

    WITH RECURSIVE walk(value) AS (
        SELECT v_root
        UNION ALL
        SELECT child.value
          FROM walk w,
               LATERAL (
                   SELECT e.value
                     FROM jsonb_each(
                              CASE WHEN jsonb_typeof(w.value) = 'object'
                                   THEN w.value ELSE '{}'::jsonb END
                          ) e
                   UNION ALL
                   SELECT e.value
                     FROM jsonb_array_elements(
                              CASE WHEN jsonb_typeof(w.value) = 'array'
                                   THEN w.value ELSE '[]'::jsonb END
                          ) e
               ) child
    )
    SELECT coalesce(array_agg(DISTINCT norm), ARRAY[]::TEXT[])
      INTO v_result
      FROM (
          SELECT platform_audit_normalize_key(k.key) AS norm
            FROM walk w,
                 LATERAL jsonb_each(
                     CASE WHEN jsonb_typeof(w.value) = 'object'
                          THEN w.value ELSE '{}'::jsonb END
                 ) k
           WHERE jsonb_typeof(w.value) = 'object'
      ) s
     WHERE norm = ANY(v_baseline);

    RETURN v_result;
END;
$$;

COMMENT ON FUNCTION platform_audit_sensitive_keys(TEXT) IS 'DB 侧的敏感键扫描器（D156 的权威集合 + D172 的驼峰归一化；与 Rust 侧 SENSITIVE_KEYS 必须逐项相等）';

-- ---------------------------------------------------------------------------
-- 8. 写入通道（SECURITY DEFINER）
-- ---------------------------------------------------------------------------
CREATE OR REPLACE FUNCTION platform_audit_append(
    p_tenant_id TEXT,
    p_actor_id TEXT,
    p_action TEXT,
    p_object_type TEXT,
    p_object_id TEXT,
    p_before TEXT,
    p_after TEXT,
    p_ip TEXT,
    p_user_agent TEXT
) RETURNS TEXT
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = public
AS $$
DECLARE
    v_prev_hash TEXT;
    v_created_at TIMESTAMPTZ;
    v_created_text TEXT;
    v_hash TEXT;
    v_id TEXT;
    v_keys TEXT[];
BEGIN
    -- 1) 敏感键**硬拒绝**（D134）：写入直接失败，**不做**「悄悄把该字段删掉再写」。
    --    ⚠️ 错误信息里**只有键名**，**没有值** —— 回显值等于再泄漏一次。
    v_keys := platform_audit_sensitive_keys(p_before);
    IF v_keys IS NOT NULL AND array_length(v_keys, 1) > 0 THEN
        RAISE EXCEPTION 'the audit payload contains sensitive keys: %',
            array_to_string(v_keys, ',')
            USING ERRCODE = 'check_violation';
    END IF;
    v_keys := platform_audit_sensitive_keys(p_after);
    IF v_keys IS NOT NULL AND array_length(v_keys, 1) > 0 THEN
        RAISE EXCEPTION 'the audit payload contains sensitive keys: %',
            array_to_string(v_keys, ',')
            USING ERRCODE = 'check_violation';
    END IF;

    -- 2) 同租户写入**串行化**（D161）：advisory lock。
    --    `hashtext` 是 32 位 → 有碰撞可能（不同租户撞同一锁 = 串行化过度，
    --    **不影响正确性**，只影响并发度）。
    PERFORM pg_advisory_xact_lock(hashtext('audit:' || p_tenant_id));

    -- 3) **拿到锁之后**用**新语句**重读最后一条（D161 的关键）。
    SELECT hash INTO v_prev_hash
      FROM platform_audit_log
     WHERE tenant_id = p_tenant_id
     ORDER BY created_at DESC, id DESC
     LIMIT 1;

    IF v_prev_hash IS NULL THEN
        -- 链基准：genesis 或上次归档那段的尾 hash（D159）。
        v_prev_hash := COALESCE(
            (SELECT head_hash FROM platform_audit_chain_state WHERE tenant_id = p_tenant_id),
            repeat('0', 64)
        );
    END IF;

    -- 4) 时间戳：**锁之后**取 `clock_timestamp()`（D143 —— 不用 `now()`：
    --    `now()` 是事务开始时间，同一事务里的多条审计会拿到同一时刻）。
    v_created_at := clock_timestamp();
    v_created_text := to_char(
        v_created_at AT TIME ZONE 'UTC',
        'YYYY-MM-DD"T"HH24:MI:SS.US"Z"'
    );

    -- 5) 哈希：覆盖**存进列里的那段文本**（D141）。
    v_hash := platform_audit_canonical_hash(
        v_prev_hash,
        p_tenant_id,
        p_actor_id,
        p_action,
        p_object_type,
        p_object_id,
        p_before,
        p_after,
        COALESCE(p_ip, ''),
        COALESCE(p_user_agent, ''),
        v_created_text
    );

    -- 6) 落库。
    v_id := gen_random_uuid()::text;
    INSERT INTO platform_audit_log (
        id, tenant_id, actor_id, action, object_type, object_id,
        "before", "after", ip, user_agent, created_at, prev_hash, hash
    ) VALUES (
        v_id, p_tenant_id, p_actor_id, p_action, p_object_type, p_object_id,
        p_before, p_after, COALESCE(p_ip, ''), COALESCE(p_user_agent, ''),
        v_created_at, v_prev_hash, v_hash
    );

    -- 7) 维护链状态（D159 + **D171**）。
    --
    --    ⚠️ `head_hash` 是「**当前在线段**的基准」= genesis 或上次归档那段的尾 hash。
    --    它**不是**链的尖端 —— D159 的「写入时更新 head_hash = 新记录的 hash」与
    --    D159 的「校验时在线表第一条的 prev_hash 必须等于 head_hash」**互相矛盾**：
    --    若把新记录的 hash 写进 head_hash，接续校验就会拿「在线段第一条的 prev_hash」
    --    与「链尖」比 → **每写一条都误报断裂**。
    --
    --    本迁移按**列的定义**（「genesis，或上次归档的尾 hash」）实现：
    --    `head_hash` **只在归档时**更新；`tip_hash`（链尖）**每次写入**更新。
    INSERT INTO platform_audit_chain_state (tenant_id, head_hash, tip_hash, updated_at)
    VALUES (p_tenant_id, repeat('0', 64), v_hash, v_created_at)
    ON CONFLICT (tenant_id) DO UPDATE
        SET tip_hash = EXCLUDED.tip_hash,
            updated_at = EXCLUDED.updated_at;

    RETURN v_id;
END;
$$;

COMMENT ON FUNCTION platform_audit_append(TEXT, TEXT, TEXT, TEXT, TEXT, TEXT, TEXT, TEXT, TEXT) IS '审计写入的**唯一**通道（SECURITY DEFINER；同租户 advisory lock 串行化；敏感键硬拒绝）';

-- ---------------------------------------------------------------------------
-- 9. 链基准读取（SECURITY DEFINER）
-- ---------------------------------------------------------------------------
--
-- 链基准在 `platform_audit_chain_state` 里；应用角色对它只读，且 RLS 把它按租户隔离。
-- 这里用 `SECURITY DEFINER` + **显式租户守卫**提供统一入口：
-- 参数里的租户必须与调用方上下文里的租户**一致**，否则报错。
CREATE OR REPLACE FUNCTION platform_audit_head_hash(p_tenant_id TEXT)
RETURNS TEXT
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = public
AS $$
DECLARE
    v_head TEXT;
BEGIN
    IF p_tenant_id IS DISTINCT FROM current_setting('app.tenant_id', true) THEN
        RAISE EXCEPTION 'the audit chain baseline tenant does not match the request context'
            USING ERRCODE = 'insufficient_privilege';
    END IF;

    SELECT head_hash INTO v_head
      FROM platform_audit_chain_state
     WHERE tenant_id = p_tenant_id;

    -- 没有记录 → genesis（64 个 0）。
    RETURN COALESCE(v_head, repeat('0', 64));
END;
$$;

COMMENT ON FUNCTION platform_audit_head_hash(TEXT) IS '某租户链的**当前基准**（genesis 或上次归档那段的尾 hash）；参数租户必须与请求上下文一致';

-- ---------------------------------------------------------------------------
-- 9.1 链尖端读取（SECURITY DEFINER，**D171**）
-- ---------------------------------------------------------------------------
--
-- `tip_hash` = 链尖（最新一条的 hash）→ **末尾截断检测**用它：
-- 在线表最后一条的 `hash` 必须 == `tip_hash`，否则「尾巴被砍了」。
--
-- 同样带**显式租户守卫**（与 `platform_audit_head_hash` 同款）——
-- `SECURITY DEFINER` 绕过 RLS，作用域必须自己限定住（D174）。
CREATE OR REPLACE FUNCTION platform_audit_tip_hash(p_tenant_id TEXT)
RETURNS TEXT
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = public
AS $$
DECLARE
    v_tip TEXT;
BEGIN
    IF p_tenant_id IS DISTINCT FROM current_setting('app.tenant_id', true) THEN
        RAISE EXCEPTION 'the audit chain tip tenant does not match the request context'
            USING ERRCODE = 'insufficient_privilege';
    END IF;

    SELECT tip_hash INTO v_tip
      FROM platform_audit_chain_state
     WHERE tenant_id = p_tenant_id;

    -- ⚠️ **没有链状态记录 → `NULL`**（不是 genesis）：
    -- 「该租户从没走过平台写入通道」与「链尖恰好是 genesis」是**两件事** ——
    -- 前者**无法**做末尾截断判定（没有基准可比），后者**能**。
    -- 校验侧看到 `NULL` → **跳过**末尾截断判定（而不是误报「被截断」）。
    RETURN v_tip;
END;
$$;

COMMENT ON FUNCTION platform_audit_tip_hash(TEXT) IS '某租户链的**尖端**（最新一条的 hash；D171 用于末尾截断检测）；参数租户必须与请求上下文一致';

-- ---------------------------------------------------------------------------
-- 10. 归档通道（SECURITY DEFINER）
-- ---------------------------------------------------------------------------
--
-- 应用角色**不能**有 DELETE 权限（有的话 append-only 立刻破功）→
-- 归档只能走这个受限的特权函数（D133）。
--
-- ⚠️ **单租户**（D158）：函数里有**显式的租户守卫** ——
-- `SECURITY DEFINER` 会绕过 RLS，所以必须**自己**把作用域限定住。
CREATE OR REPLACE FUNCTION platform_audit_archive_move(
    p_tenant_id TEXT,
    p_before TIMESTAMPTZ
) RETURNS BIGINT
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = public
AS $$
DECLARE
    v_tail_hash TEXT;
    v_archived_at TIMESTAMPTZ := clock_timestamp();
    v_moved BIGINT := 0;
BEGIN
    -- 租户守卫：只允许搬走「调用方上下文里的租户」的行。
    IF p_tenant_id IS DISTINCT FROM current_setting('app.tenant_id', true) THEN
        RAISE EXCEPTION 'the audit archive tenant does not match the request context'
            USING ERRCODE = 'insufficient_privilege';
    END IF;

    -- 被搬走那段的**尾 hash**（更新链基准用，D159）。
    SELECT hash INTO v_tail_hash
      FROM platform_audit_log
     WHERE tenant_id = p_tenant_id
       AND created_at < p_before
     ORDER BY created_at DESC, id DESC
     LIMIT 1;

    IF v_tail_hash IS NULL THEN
        -- 没有可搬的行 → **幂等**（影响 0 行，不报错，AC-S-131 步骤 3）。
        RETURN 0;
    END IF;

    WITH moved AS (
        DELETE FROM platform_audit_log
         WHERE tenant_id = p_tenant_id
           AND created_at < p_before
        RETURNING id, tenant_id, actor_id, action, object_type, object_id,
                  "before", "after", ip, user_agent, created_at, prev_hash, hash
    )
    INSERT INTO platform_audit_archive (
        id, tenant_id, actor_id, action, object_type, object_id,
        "before", "after", ip, user_agent, created_at, prev_hash, hash, archived_at
    )
    SELECT id, tenant_id, actor_id, action, object_type, object_id,
           "before", "after", ip, user_agent, created_at, prev_hash, hash, v_archived_at
      FROM moved
    ON CONFLICT (id) DO NOTHING;

    GET DIAGNOSTICS v_moved = ROW_COUNT;

    -- 更新链状态：`head_hash` = 被搬走那段的尾 hash（D159）；
    -- `tip_hash` = **链尖**（最新一条的 hash，D171）。
    --
    -- ⚠️ 归档**不改变**链尖，除非**在线表被搬空** —— 那时链尖就是被搬走那段的尾 hash
    --    （= 新的 `head_hash`）。所以 `tip_hash` 取「在线表剩下的最后一条」，
    --    在线表空了则退回 `v_tail_hash`。
    INSERT INTO platform_audit_chain_state (tenant_id, head_hash, tip_hash, archived_at, updated_at)
    VALUES (
        p_tenant_id,
        v_tail_hash,
        COALESCE(
            (SELECT hash
               FROM platform_audit_log
              WHERE tenant_id = p_tenant_id
              ORDER BY created_at DESC, id DESC
              LIMIT 1),
            v_tail_hash
        ),
        v_archived_at,
        v_archived_at
    )
    ON CONFLICT (tenant_id) DO UPDATE
        SET head_hash = EXCLUDED.head_hash,
            tip_hash = EXCLUDED.tip_hash,
            archived_at = EXCLUDED.archived_at,
            updated_at = EXCLUDED.updated_at;

    RETURN v_moved;
END;
$$;

COMMENT ON FUNCTION platform_audit_archive_move(TEXT, TIMESTAMPTZ) IS '把超期记录搬进归档表（SECURITY DEFINER；单租户；幂等；更新链基准）';

-- ---------------------------------------------------------------------------
-- 11. 函数授权（默认不给 PUBLIC）
-- ---------------------------------------------------------------------------
REVOKE ALL ON FUNCTION platform_audit_canonical_hash(TEXT, TEXT, TEXT, TEXT, TEXT, TEXT, TEXT, TEXT, TEXT, TEXT, TEXT) FROM PUBLIC;
REVOKE ALL ON FUNCTION platform_audit_normalize_key(TEXT) FROM PUBLIC;
REVOKE ALL ON FUNCTION platform_audit_sensitive_keys(TEXT) FROM PUBLIC;
REVOKE ALL ON FUNCTION platform_audit_append(TEXT, TEXT, TEXT, TEXT, TEXT, TEXT, TEXT, TEXT, TEXT) FROM PUBLIC;
REVOKE ALL ON FUNCTION platform_audit_head_hash(TEXT) FROM PUBLIC;
REVOKE ALL ON FUNCTION platform_audit_tip_hash(TEXT) FROM PUBLIC;
REVOKE ALL ON FUNCTION platform_audit_archive_move(TEXT, TIMESTAMPTZ) FROM PUBLIC;

DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'base_platform_app') THEN
        EXECUTE 'GRANT EXECUTE ON FUNCTION platform_audit_append(TEXT, TEXT, TEXT, TEXT, TEXT, TEXT, TEXT, TEXT, TEXT) TO base_platform_app';
        EXECUTE 'GRANT EXECUTE ON FUNCTION platform_audit_head_hash(TEXT) TO base_platform_app';
        EXECUTE 'GRANT EXECUTE ON FUNCTION platform_audit_tip_hash(TEXT) TO base_platform_app';
        EXECUTE 'GRANT EXECUTE ON FUNCTION platform_audit_archive_move(TEXT, TIMESTAMPTZ) TO base_platform_app';
    END IF;
END
$$;
