-- 回滚 QA-1 的加固：把两个函数**还原成 `20261120000200_audit` 的版本**。
--
-- ⚠️ 回滚意味着**重新打开**两个缺口：
-- 1. `platform_audit_archive_move` 不再持 advisory lock → 并发归档会覆盖 `tip_hash`；
-- 2. `platform_audit_append` 不再校验租户 → 可跨租户写审计。
--
-- → 只在「确认生产从未归档过、且没有跨租户调用」的紧急场景使用。

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

-- ---------------------------------------------------------------------------
-- 还原**函数的授权**（`CREATE OR REPLACE` 保留原 ACL，但这里显式重申更稳妥）
-- ---------------------------------------------------------------------------

-- ⚠️ `CREATE OR REPLACE FUNCTION` **不清除**已有授权，
-- 但若本迁移在**全新库**上先于 `20261120000200_audit` 应用（不可能，版本号大），
-- 显式 REVOKE 能保证「应用角色只能通过这两个函数写审计」这条不变量。
REVOKE ALL ON FUNCTION platform_audit_append(TEXT, TEXT, TEXT, TEXT, TEXT, TEXT, TEXT, TEXT, TEXT) FROM PUBLIC;
REVOKE ALL ON FUNCTION platform_audit_archive_move(TEXT, TIMESTAMPTZ) FROM PUBLIC;

COMMENT ON FUNCTION platform_audit_append(TEXT, TEXT, TEXT, TEXT, TEXT, TEXT, TEXT, TEXT, TEXT) IS '审计写入的**唯一**通道（SECURITY DEFINER；**租户守卫** + 同租户 advisory lock 串行化 + 敏感键硬拒绝）';
COMMENT ON FUNCTION platform_audit_archive_move(TEXT, TIMESTAMPTZ) IS '把超期记录搬进归档表（SECURITY DEFINER；单租户 + **同租户 advisory lock**；幂等；更新链基准）';
