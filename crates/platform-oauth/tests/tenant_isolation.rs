//! AC-S-155：OAuth 数据**跨租户不可见**（**DB 层**）。
//!
//! # 为什么必须在 **DB 层**验（而不是只看 API）
//!
//! API 层的「只看得到自己的」有**两**道防线：
//!
//! 1. **应用层**：每条语句都带 `tenant_id = {tenant}`；
//! 2. **数据库层**：4 张表**启用 RLS**（policy 由 `cargo xtask tenant-policies`
//!    依 manifest 的 `tenantScoped` 段生成，D227）。
//!
//! **只**测 API → 少测了第 2 道 → 有人把 RLS 关掉、或漏掉一张表，**测不出来**。
//!
//! # ⚠️ 连接身份
//!
//! - **读**：用**应用角色** `base_app`（不是超管）—— 超管**绕过 RLS** → 隔离
//!   看起来「正常」其实是 fail open（REQ-014 的教训）；
//! - **造数**：用**管理员**（超管）→ 两边都能看见，才证明「看得见是因为该看见」。

mod common;

use common::{TENANT_A, TENANT_B, app_role_url, required_test_database_url, setup};
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;

/// 造数用的**管理员**连接（超管 → 绕过 RLS，只用于夹具）。
async fn admin_pool() -> PgPool {
    PgPoolOptions::new()
        .max_connections(2)
        .connect(&required_test_database_url())
        .await
        .expect("管理员连接应当能建立")
}

/// 应用角色的连接（**不**绕过 RLS）。
async fn app_pool() -> PgPool {
    PgPoolOptions::new()
        .max_connections(2)
        .connect(&app_role_url(&required_test_database_url()))
        .await
        .expect("应用角色连接应当能建立")
}

/// 造 4 张表 × 2 个租户的行（管理员直连 → 两边都写得到）。
async fn seed_rows(admin: &PgPool) {
    for (tenant, tag) in [(TENANT_A, "a"), (TENANT_B, "b")] {
        sqlx::query(
            "INSERT INTO platform_oauth_grant (tenant_id, id, client_id, user_id, scopes, \
             status, granted_at, consent_at) \
             VALUES ($1, $2, 'client-x', $3, ARRAY['rbac:role:read'], 'active', now(), now())",
        )
        .bind(tenant)
        .bind(format!("grant-{tag}"))
        .bind(format!("user-{tag}"))
        .execute(admin)
        .await
        .expect("grant 夹具应当能插入");

        sqlx::query(
            "INSERT INTO platform_oauth_code (tenant_id, id, client_id, user_id, code_hash, \
             redirect_uri, scope, code_challenge, code_challenge_method, expires_at) \
             VALUES ($1, $2, 'client-x', $3, $4, 'https://client.example/cb', 'rbac:role:read', \
             'chal', 'S256', now() + interval '60 seconds')",
        )
        .bind(tenant)
        .bind(format!("code-{tag}"))
        .bind(format!("user-{tag}"))
        .bind(format!("hash-{tag}"))
        .execute(admin)
        .await
        .expect("code 夹具应当能插入");

        sqlx::query(
            "INSERT INTO platform_oauth_device_code (tenant_id, id, client_id, \
             device_code_hash, user_code_hash, scope, status, device_code_expires_at, \
             user_code_expires_at, poll_interval_seconds) \
             VALUES ($1, $2, 'client-x', $3, $4, 'rbac:role:read', 'pending', \
             now() + interval '600 seconds', now() + interval '600 seconds', 5)",
        )
        .bind(tenant)
        .bind(format!("device-{tag}"))
        .bind(format!("dhash-{tag}"))
        .bind(format!("uhash-{tag}"))
        .execute(admin)
        .await
        .expect("device_code 夹具应当能插入");

        sqlx::query(
            "INSERT INTO platform_oauth_token (tenant_id, id, client_id, user_id, token_id, \
             scopes, grant_id, status, issued_at, expires_at) \
             VALUES ($1, $2, 'client-x', $3, $4, ARRAY['rbac:role:read'], NULL, 'active', \
             now(), now() + interval '3600 seconds')",
        )
        .bind(tenant)
        .bind(format!("token-{tag}"))
        .bind(format!("user-{tag}"))
        .bind(format!("jti-{tag}"))
        .execute(admin)
        .await
        .expect("token 夹具应当能插入");
    }
}

/// 在**应用角色**的连接里，于**单个事务**内设租户上下文 → 数 4 张表的行数。
///
/// ⚠️ `set_config(..., true)` = **事务局部**（D148）→ 事务一提交就失效。
async fn counts_in_tenant(app: &PgPool, tenant: Option<&str>) -> Vec<i64> {
    let mut tx = app.begin().await.expect("事务应当能开");
    if let Some(tenant) = tenant {
        sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
            .bind(tenant)
            .execute(&mut *tx)
            .await
            .expect("租户上下文应当能设");
    }
    let mut counts = Vec::new();
    for table in [
        "platform_oauth_grant",
        "platform_oauth_code",
        "platform_oauth_device_code",
        "platform_oauth_token",
    ] {
        let count: i64 = sqlx::query_scalar(&format!("SELECT count(*) FROM {table}"))
            .fetch_one(&mut *tx)
            .await
            .expect("应当能数行数");
        counts.push(count);
    }
    tx.commit().await.expect("事务应当能提交");
    counts
}

/// AC-S-155：4 张租户表**跨租户不可见**；写侧也被 RLS 挡住。
#[tokio::test]
async fn the_oauth_tables_are_isolated_by_tenant() {
    let _fixture = setup().await;
    let admin = admin_pool().await;
    let app = app_pool().await;
    seed_rows(&admin).await;

    // 步骤 1：租户 a 的上下文 → 只看得到 tenant-a 的行（每张表 1 行）。
    assert_eq!(
        counts_in_tenant(&app, Some(TENANT_A)).await,
        vec![1, 1, 1, 1],
        "tenant-a 应当只看到自己的行"
    );
    // 步骤 2：租户 b 的上下文 → 只看得到 tenant-b 的行。
    assert_eq!(
        counts_in_tenant(&app, Some(TENANT_B)).await,
        vec![1, 1, 1, 1],
        "tenant-b 应当只看到自己的行"
    );
    // 步骤 3：**超管**看得见两个租户的全部行（证明步骤 1/2 不是「表本来就空」）。
    let superuser: i64 = sqlx::query_scalar("SELECT count(*) FROM platform_oauth_grant")
        .fetch_one(&admin)
        .await
        .expect("超管应当能数行数");
    assert_eq!(superuser, 2, "超管应当看得见两个租户的行");

    // 步骤 4：**没有**租户上下文 → 0 行（fail closed）。
    assert_eq!(
        counts_in_tenant(&app, None).await,
        vec![0, 0, 0, 0],
        "没有租户上下文时应当一行都看不见"
    );

    // 步骤 6（AC-S-155）：`pg_policies` 里 4 张表**各有 1 条**
    // `<table>_tenant_isolation` policy —— **名字与条数都要对**。
    //
    // ⚠️ 只断 `relrowsecurity` 是不够的：policy 被**改名**、或**多加一条宽松 policy**
    // （第二条 `USING (true)` 会把隔离彻底放开）当前都测不出来。
    let expected = [
        "platform_oauth_grant",
        "platform_oauth_token",
        "platform_oauth_code",
        "platform_oauth_device_code",
    ];
    for table in expected {
        let policies: Vec<String> = sqlx::query_scalar(
            "SELECT policyname FROM pg_policies WHERE schemaname = 'public' AND tablename = $1 \
             ORDER BY policyname",
        )
        .bind(table)
        .fetch_all(&admin)
        .await
        .expect("应当能查 pg_policies");
        assert_eq!(
            policies,
            vec![format!("{table}_tenant_isolation")],
            "{table} 必须**恰好 1 条** `<table>_tenant_isolation` policy：{policies:?}"
        );
    }

    // 步骤 5：写侧 —— `WITH CHECK` 挡住「往别的租户插行」（D247）。
    //
    // ⚠️ **两条 INSERT 必须用两个事务**：PostgreSQL 里一条语句报错后，
    // **整个事务进入 aborted 状态** → 后续语句都失败（"current transaction is
    // aborted"）→ 放在同一个事务里就分不清「被 RLS 拒了」和「事务已经废了」。
    let mut tx = app.begin().await.expect("事务应当能开");
    sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
        .bind(TENANT_A)
        .execute(&mut *tx)
        .await
        .expect("租户上下文应当能设");
    let cross = sqlx::query(
        "INSERT INTO platform_oauth_grant (tenant_id, id, client_id, user_id, scopes, status, \
         granted_at) VALUES ('tenant-b', 'cross-tenant', 'client-x', 'user-x', \
         ARRAY['rbac:role:read'], 'active', now())",
    )
    .execute(&mut *tx)
    .await;
    assert!(
        cross.is_err(),
        "往别的租户插行必须被 RLS 的 WITH CHECK 拒掉"
    );
    tx.rollback().await.expect("回滚应当能执行");

    // 自己的租户 → 成功（**新**事务：上一个已经 aborted）。
    let mut tx = app.begin().await.expect("事务应当能开");
    sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
        .bind(TENANT_A)
        .execute(&mut *tx)
        .await
        .expect("租户上下文应当能设");
    let same = sqlx::query(
        "INSERT INTO platform_oauth_grant (tenant_id, id, client_id, user_id, scopes, status, \
         granted_at) VALUES ('tenant-a', 'same-tenant', 'client-x', 'user-x', \
         ARRAY['rbac:role:read'], 'active', now())",
    )
    .execute(&mut *tx)
    .await;
    assert!(same.is_ok(), "往自己的租户插行必须成功");
    tx.rollback().await.expect("回滚应当能执行");
}

/// AC-S-155 的**补充判据**：`platform_oauth_client` 是**平台级**表
/// （**无** `tenant_id`、**不启用** RLS，D219）。
#[tokio::test]
async fn the_oauth_client_table_is_platform_scoped() {
    let _fixture = setup().await;
    let admin = admin_pool().await;
    let app = app_pool().await;

    // 表里**没有** `tenant_id` 列。
    let columns: Vec<String> = sqlx::query_scalar(
        "SELECT column_name FROM information_schema.columns \
         WHERE table_name = 'platform_oauth_client'",
    )
    .fetch_all(&admin)
    .await
    .expect("应当能查列集合");
    assert!(
        !columns.iter().any(|column| column == "tenant_id"),
        "platform_oauth_client 是平台级表 → 不该有 tenant_id 列：{columns:?}"
    );

    // **不启用** RLS。
    let enabled: bool = sqlx::query_scalar(
        "SELECT relrowsecurity FROM pg_class WHERE relname = 'platform_oauth_client'",
    )
    .fetch_one(&admin)
    .await
    .expect("应当能查 relrowsecurity");
    assert!(!enabled, "platform_oauth_client 不该启用 RLS");

    // 插一行 → **任何**租户的上下文都看得见（平台级资产）。
    sqlx::query(
        "INSERT INTO platform_oauth_client (client_id, client_type, name, redirect_uris, \
         allowed_grant_types, allowed_scopes, status, created_at) \
         VALUES ('platform-scoped-client', 'public', '平台级', ARRAY['https://client.example/cb'], \
         ARRAY['authorization_code'], ARRAY['rbac:role:read'], 'active', now())",
    )
    .execute(&admin)
    .await
    .expect("平台级 client 应当能插入");

    for tenant in [TENANT_A, TENANT_B] {
        let mut tx = app.begin().await.expect("事务应当能开");
        sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
            .bind(tenant)
            .execute(&mut *tx)
            .await
            .expect("租户上下文应当能设");
        let count: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM platform_oauth_client WHERE client_id = 'platform-scoped-client'",
        )
        .fetch_one(&mut *tx)
        .await
        .expect("应当能数行数");
        tx.commit().await.expect("事务应当能提交");
        assert_eq!(
            count, 1,
            "平台级 client 对每个租户都可见（tenant={tenant}）"
        );
    }
}
