-- REQ-018 存储与导入导出的 4 张表（design §2 / D319~D332）。
--
-- # 4 张表
--
-- | 表 | 租户维度 | 作用 |
-- |----|---------|------|
-- | `platform_storage_object`    | **租户级** | 对象元数据（已落存储的文件） |
-- | `platform_storage_upload`    | **租户级** | 分片上传会话（可续传） |
-- | `platform_storage_job`       | **租户级** | 导入/导出任务与进度 |
-- | `platform_storage_job_error` | **租户级** | 行级错误回执 |
--
-- # 为什么拆 4 张而不是合 1 张
--
-- 生命周期完全不同：TTL 扫描器只碰 `upload`、预签名只读 `object`、
-- 轮询器只扫 `job`。合表后三类高频访问会在同一批堆页上互相抬升冲突，
-- 且 `job_error` 是「一任务 N 行」的放大表，与任务本身 1:1 混存会让
-- 任务列表的索引被错误行撑爆。
--
-- # ⚠️ RLS policy **不在本迁移里手写**（同 REQ-015 的 D227）
--
-- 策略由 `cargo xtask tenant-policies` 从
-- `crates/platform-storage/manifest.json` 的 `tenantScoped` 段生成 ——
-- 手写 policy 迟早漏一张，而漏一张 = 该表**完全无隔离**。

-- ---------------------------------------------------------------------------
-- 1. 对象元数据
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS platform_storage_object (
    id            UUID        PRIMARY KEY,
    tenant_id     TEXT        NOT NULL,
    -- 逻辑路径（`docs/a.txt`）；**租户内唯一**，跨租户可重名。
    object_key    TEXT        NOT NULL,
    size_bytes    BIGINT      NOT NULL,
    -- 内容摘要：合并分片后与 `declared_checksum` 比对（D322）。
    sha256        TEXT        NOT NULL,
    content_type  TEXT        NOT NULL,
    -- `skipped` / `clean` / `infected`（D326）。**用 TEXT 不用 enum**：
    -- 新增判定值不必再开一次迁移窗口。
    scan_verdict  TEXT        NOT NULL,
    status        TEXT        NOT NULL,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT platform_storage_object_tenant_key_key UNIQUE (tenant_id, object_key)
);

-- ---------------------------------------------------------------------------
-- 2. 上传会话
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS platform_storage_upload (
    upload_id         UUID        PRIMARY KEY,
    tenant_id         TEXT        NOT NULL,
    object_key        TEXT        NOT NULL,
    total_parts       INTEGER     NOT NULL,
    part_size         BIGINT      NOT NULL,
    -- 客户端声明的摘要，**可为空**：流式上传且客户端未预计算时省略，
    -- 由 `complete` 阶段按服务端侧计算值兜底比对。
    declared_checksum TEXT        NULL,
    status            TEXT        NOT NULL,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- 驱动 TTL 扫描：过期未完成会话被回收，而不是永久占位。
    expires_at        TIMESTAMPTZ NOT NULL
);

-- ---------------------------------------------------------------------------
-- 3. 导入/导出任务
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS platform_storage_job (
    id             UUID        PRIMARY KEY,
    tenant_id      TEXT        NOT NULL,
    -- `import` / `export`。
    kind           TEXT        NOT NULL,
    entity         TEXT        NOT NULL,
    status         TEXT        NOT NULL,
    total_rows     BIGINT      NOT NULL,
    processed_rows BIGINT      NOT NULL,
    failed_rows    BIGINT      NOT NULL,
    -- 导出任务记录产物对象；导入任务为来源对象；无对象形态时为 NULL。
    object_key     TEXT        NULL,
    -- **任务级**失败原因；行级原因在下一张表（两者不可混）。
    error          TEXT        NULL,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- 未结束时为 NULL；轮询器据此判断是否需要继续取任务。
    finished_at    TIMESTAMPTZ NULL
);

-- ---------------------------------------------------------------------------
-- 4. 行级错误回执
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS platform_storage_job_error (
    job_id     UUID   NOT NULL,
    tenant_id  TEXT   NOT NULL,
    row_number BIGINT NOT NULL,
    -- ⚠️ `column` 是 SQL 保留字：建表与所有引用处**必须**写成 "column"，
    -- 否则解析期就报语法错（不是运行时才有事）。
    "column"   TEXT   NOT NULL,
    reason     TEXT   NOT NULL,
    -- 同一行同一列只会失败一次：批处理重跑时覆盖而不是叠加。
    PRIMARY KEY (job_id, row_number, "column")
);

-- 错误回执永远按「租户 + 任务 + 行号」读取，且要保序。
CREATE INDEX IF NOT EXISTS platform_storage_job_error_tenant_job_row_idx
    ON platform_storage_job_error (tenant_id, job_id, row_number);

-- 轮询器按「租户 + 状态」取活跃任务，新的在前。
CREATE INDEX IF NOT EXISTS platform_storage_job_tenant_status_created_idx
    ON platform_storage_job (tenant_id, status, created_at);

-- TTL 扫描器按「租户 + 状态」取未完成会话，按过期时间先后处理。
CREATE INDEX IF NOT EXISTS platform_storage_upload_tenant_status_expires_idx
    ON platform_storage_upload (tenant_id, status, expires_at);

-- ---------------------------------------------------------------------------
-- 5. 应用角色授权（**新表必须显式 GRANT**，不会自动继承）
-- ---------------------------------------------------------------------------
DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'base_platform_app') THEN
        EXECUTE 'GRANT SELECT, INSERT, UPDATE, DELETE ON platform_storage_object TO base_platform_app';
        EXECUTE 'GRANT SELECT, INSERT, UPDATE, DELETE ON platform_storage_upload TO base_platform_app';
        EXECUTE 'GRANT SELECT, INSERT, UPDATE, DELETE ON platform_storage_job TO base_platform_app';
        EXECUTE 'GRANT SELECT, INSERT, UPDATE, DELETE ON platform_storage_job_error TO base_platform_app';
    END IF;
END
$$;
