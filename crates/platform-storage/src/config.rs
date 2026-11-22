//! 文件存储的**运行期配置**（REQ-018，design §6 / D331）。
//!
//! # 6 个配置键（键名 **lowerCamelCase**，契约 C9）
//!
//! | 键 | 默认 | 说明 |
//! |----|------|------|
//! | `storage.maxUploadBytes` | `104857600` | 单文件上限（100 MiB） |
//! | `storage.partSizeBytes` | `8388608` | 分片大小（8 MiB） |
//! | `storage.presignTtlSeconds` | `900` | 预签名有效期（15 分钟） |
//! | `storage.importBatchSize` | `1000` | 导入每批行数 |
//! | `storage.maxRowBytes` | `65536` | 单行上限（D325） |
//! | `storage.uploadSessionTtlSeconds` | `86400` | 未完成上传会话的超时 |
//!
//! ⚠️ **每个键必须带 `"scope": "platform"`**（REQ-016 的 S-01 教训：
//! `ConfigSpec.scope` 的 serde 默认值是 `Tenant` → 漏写就落到租户作用域 →
//! 平台级读写端点直接拒 → 配置改了**永不生效**）。
//! ⚠️ 键名段内**不得带 `_`**（`ConfigKey::parse` 要求小写字母开头 + 只含字母数字
//! → 带 `_` 会让服务起不来）。
//!
//! # ⚠️ 配置**每请求读**（不是装配时读一次）
//!
//! 装配时读一次 = 配置改了**永远不生效**（除非重启）。
//! 用 trait 接缝 [`StorageConfigSource`]，实现由装配层注入。

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use platform_core::RequestContext;

/// 装箱的 future（让 trait 保持**可对象化**）。
pub type StorageBoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// 单文件上限（字节）。
pub const CONFIG_KEY_MAX_UPLOAD_BYTES: &str = "storage.maxUploadBytes";
/// 分片大小（字节）。
pub const CONFIG_KEY_PART_SIZE_BYTES: &str = "storage.partSizeBytes";
/// 预签名有效期（秒）。
pub const CONFIG_KEY_PRESIGN_TTL_SECONDS: &str = "storage.presignTtlSeconds";
/// 导入每批行数。
pub const CONFIG_KEY_IMPORT_BATCH_SIZE: &str = "storage.importBatchSize";
/// 单行上限（字节）。
pub const CONFIG_KEY_MAX_ROW_BYTES: &str = "storage.maxRowBytes";
/// 未完成上传会话的超时（秒）。
pub const CONFIG_KEY_UPLOAD_SESSION_TTL_SECONDS: &str = "storage.uploadSessionTtlSeconds";

/// 6 个配置键（AC-S-184 断言 manifest 里恰好这 6 个）。
pub const CONFIG_KEYS: &[&str] = &[
    CONFIG_KEY_MAX_UPLOAD_BYTES,
    CONFIG_KEY_PART_SIZE_BYTES,
    CONFIG_KEY_PRESIGN_TTL_SECONDS,
    CONFIG_KEY_IMPORT_BATCH_SIZE,
    CONFIG_KEY_MAX_ROW_BYTES,
    CONFIG_KEY_UPLOAD_SESSION_TTL_SECONDS,
];

/// 默认值（D331）。
pub mod defaults {
    /// 单文件上限。
    pub const MAX_UPLOAD_BYTES: i64 = 104_857_600;
    /// 分片大小。
    pub const PART_SIZE_BYTES: i64 = 8_388_608;
    /// 预签名有效期。
    pub const PRESIGN_TTL_SECONDS: i64 = 900;
    /// 导入批大小。
    pub const IMPORT_BATCH_SIZE: i64 = 1_000;
    /// 单行上限。
    pub const MAX_ROW_BYTES: i64 = 65_536;
    /// 上传会话超时。
    pub const UPLOAD_SESSION_TTL_SECONDS: i64 = 86_400;
}

/// 装配后的 6 个配置值。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StorageSettings {
    /// 单文件上限（字节）。
    pub max_upload_bytes: i64,
    /// 分片大小（字节）。
    pub part_size_bytes: i64,
    /// 预签名有效期（秒）。
    pub presign_ttl_seconds: i64,
    /// 导入每批行数。
    pub import_batch_size: i64,
    /// 单行上限（字节）。
    pub max_row_bytes: i64,
    /// 上传会话超时（秒）。
    pub upload_session_ttl_seconds: i64,
}

impl Default for StorageSettings {
    fn default() -> Self {
        Self {
            max_upload_bytes: defaults::MAX_UPLOAD_BYTES,
            part_size_bytes: defaults::PART_SIZE_BYTES,
            presign_ttl_seconds: defaults::PRESIGN_TTL_SECONDS,
            import_batch_size: defaults::IMPORT_BATCH_SIZE,
            max_row_bytes: defaults::MAX_ROW_BYTES,
            upload_session_ttl_seconds: defaults::UPLOAD_SESSION_TTL_SECONDS,
        }
    }
}

/// 配置读取的接缝（由装配层注入实现）。
pub trait StorageConfigSource: Send + Sync {
    /// 读一个平台级配置项的值（`None` = 未配置 → 用默认值）。
    fn read_platform<'a>(
        &'a self,
        ctx: &'a RequestContext,
        key: &'a str,
    ) -> StorageBoxFuture<'a, Option<i64>>;
}

/// 装配后的配置（每个请求读一次）。
#[derive(Clone)]
pub struct StorageConfig {
    source: Arc<dyn StorageConfigSource>,
}

impl StorageConfig {
    /// 用配置源构造。
    pub fn new(source: Arc<dyn StorageConfigSource>) -> Self {
        Self { source }
    }

    /// 读 6 个配置值（缺省用 [`StorageSettings::default`]）。
    pub async fn settings(&self, ctx: &RequestContext) -> StorageSettings {
        let d = StorageSettings::default();
        StorageSettings {
            max_upload_bytes: self
                .read(ctx, CONFIG_KEY_MAX_UPLOAD_BYTES)
                .await
                .unwrap_or(d.max_upload_bytes),
            part_size_bytes: self
                .read(ctx, CONFIG_KEY_PART_SIZE_BYTES)
                .await
                .unwrap_or(d.part_size_bytes),
            presign_ttl_seconds: self
                .read(ctx, CONFIG_KEY_PRESIGN_TTL_SECONDS)
                .await
                .unwrap_or(d.presign_ttl_seconds),
            import_batch_size: self
                .read(ctx, CONFIG_KEY_IMPORT_BATCH_SIZE)
                .await
                .unwrap_or(d.import_batch_size),
            max_row_bytes: self
                .read(ctx, CONFIG_KEY_MAX_ROW_BYTES)
                .await
                .unwrap_or(d.max_row_bytes),
            upload_session_ttl_seconds: self
                .read(ctx, CONFIG_KEY_UPLOAD_SESSION_TTL_SECONDS)
                .await
                .unwrap_or(d.upload_session_ttl_seconds),
        }
    }

    async fn read(&self, ctx: &RequestContext, key: &str) -> Option<i64> {
        self.source.read_platform(ctx, key).await
    }
}

// ---------------------------------------------------------------------------
// 存储根目录（环境变量，不走配置表）
// ---------------------------------------------------------------------------

/// 本地存储根目录的环境变量。
///
/// ⚠️ **走环境变量而不是配置表**：它是**部署事实**（每台机器的路径不同），
/// 不是「业务上可调的值」。写进 `platform_config` 会让「换个部署」变成「改配置」，
/// 而且配置文件里出现宿主路径是反模式。
pub const STORAGE_ROOT_ENV: &str = "BASE_STORAGE_ROOT";

/// 预签名 HMAC 密钥的环境变量。
///
/// ⚠️ **敏感值，永不进日志**（与 SMTP 密码同款纪律，D306）。
pub const PRESIGN_SECRET_ENV: &str = "BASE_STORAGE_PRESIGN_SECRET";

/// 预签名密钥的标识（用于轮换：泄露时能按 id 失效）。
pub const PRESIGN_KEY_ID_ENV: &str = "BASE_STORAGE_PRESIGN_KEY_ID";

/// 从环境变量读本地存储配置。
///
/// 返回 `None` 表示**未配置** → 装配层应把 `LocalFsStore` 标为不可用
/// （而不是 panic —— 见 `main.rs` 的装配逻辑）。
#[derive(Clone, PartialEq, Eq)]
pub struct LocalStorageEnv {
    /// 存储根目录。
    pub root: String,
    /// 预签名密钥（**敏感**）。
    pub presign_secret: String,
    /// 密钥标识。
    pub presign_key_id: String,
}

impl LocalStorageEnv {
    /// 从「查环境变量」的函数构造。
    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Option<Self> {
        let root = non_empty(lookup(STORAGE_ROOT_ENV))?;
        let presign_secret = non_empty(lookup(PRESIGN_SECRET_ENV))?;
        let presign_key_id = non_empty(lookup(PRESIGN_KEY_ID_ENV))?;
        Some(Self {
            root,
            presign_secret,
            presign_key_id,
        })
    }

    /// 从真实环境变量构造。
    pub fn from_env() -> Option<Self> {
        Self::from_lookup(|key| std::env::var(key).ok())
    }
}

impl std::fmt::Debug for LocalStorageEnv {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // ⚠️ 密钥固定打码（与 `SmtpConfig` / `S3Config` 同款纪律）。
        f.debug_struct("LocalStorageEnv")
            .field("root", &self.root)
            .field("presign_secret", &"***")
            .field("presign_key_id", &self.presign_key_id)
            .finish()
    }
}

fn non_empty(value: Option<String>) -> Option<String> {
    value.filter(|v| !v.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    /// 6 个键恰好 6 个且无重复。
    #[test]
    fn there_are_exactly_six_config_keys() {
        assert_eq!(CONFIG_KEYS.len(), 6, "D331 把基线钉死为 6 个");
        let unique: std::collections::BTreeSet<&str> = CONFIG_KEYS.iter().copied().collect();
        assert_eq!(unique.len(), 6);
    }

    /// 每段的 lowerCamelCase 检查（段内带 `_` 会让服务起不来）。
    #[test]
    fn every_config_key_segment_is_lower_camel_case() {
        for key in CONFIG_KEYS {
            for segment in key.split('.') {
                assert!(!segment.is_empty(), "键 {key} 有空段");
                let first = segment.chars().next().expect("非空");
                assert!(
                    first.is_ascii_lowercase(),
                    "键 {key} 的段 {segment} 必须以小写字母开头"
                );
                assert!(
                    segment.chars().all(|c| c.is_ascii_alphanumeric()),
                    "键 {key} 的段 {segment} 只能含 ASCII 字母数字（带 _ 会让服务起不来）"
                );
            }
        }
    }

    /// 全部键在 `storage.` 命名空间下。
    #[test]
    fn every_config_key_is_in_the_storage_namespace() {
        for key in CONFIG_KEYS {
            assert!(key.starts_with("storage."), "键必须在 storage. 下：{key}");
        }
    }

    /// 默认值与 D331 逐字一致（6 个都要断言 —— §9.15.2）。
    #[test]
    fn the_defaults_match_the_spec() {
        let d = StorageSettings::default();
        assert_eq!(d.max_upload_bytes, 104_857_600);
        assert_eq!(d.part_size_bytes, 8_388_608);
        assert_eq!(d.presign_ttl_seconds, 900);
        assert_eq!(d.import_batch_size, 1_000);
        assert_eq!(d.max_row_bytes, 65_536);
        assert_eq!(d.upload_session_ttl_seconds, 86_400);
    }

    /// 默认值常量与 `StorageSettings::default()` 一致（防两处漂移）。
    #[test]
    fn the_default_constants_agree_with_the_struct_default() {
        let d = StorageSettings::default();
        assert_eq!(d.max_upload_bytes, defaults::MAX_UPLOAD_BYTES);
        assert_eq!(d.part_size_bytes, defaults::PART_SIZE_BYTES);
        assert_eq!(d.presign_ttl_seconds, defaults::PRESIGN_TTL_SECONDS);
        assert_eq!(d.import_batch_size, defaults::IMPORT_BATCH_SIZE);
        assert_eq!(d.max_row_bytes, defaults::MAX_ROW_BYTES);
        assert_eq!(
            d.upload_session_ttl_seconds,
            defaults::UPLOAD_SESSION_TTL_SECONDS
        );
    }

    // -- 环境变量 ------------------------------------------------------------

    fn env_map() -> BTreeMap<&'static str, String> {
        BTreeMap::from([
            (STORAGE_ROOT_ENV, "/var/lib/base/storage".to_string()),
            (PRESIGN_SECRET_ENV, "super-secret-value".to_string()),
            (PRESIGN_KEY_ID_ENV, "k1".to_string()),
        ])
    }

    /// 3 个环境变量全有 → 能构造。
    #[test]
    fn the_local_env_builds_when_all_three_are_present() {
        let env = env_map();
        let cfg = LocalStorageEnv::from_lookup(|k| env.get(k).cloned()).expect("应能构造");
        assert_eq!(cfg.root, "/var/lib/base/storage");
        assert_eq!(cfg.presign_key_id, "k1");
    }

    /// 缺任一 → `None`（未配置）。
    #[test]
    fn the_local_env_is_none_when_any_variable_is_missing() {
        for missing in [STORAGE_ROOT_ENV, PRESIGN_SECRET_ENV, PRESIGN_KEY_ID_ENV] {
            let mut env = env_map();
            env.remove(missing);
            assert!(
                LocalStorageEnv::from_lookup(|k| env.get(k).cloned()).is_none(),
                "缺 {missing} 时必须是 None"
            );
        }
    }

    /// 空白值也算未配置。
    #[test]
    fn the_local_env_treats_blank_as_missing() {
        let mut env = env_map();
        env.insert(PRESIGN_SECRET_ENV, "   ".to_string());
        assert!(LocalStorageEnv::from_lookup(|k| env.get(k).cloned()).is_none());
    }

    /// **`Debug` 必须打码密钥**（与 SmtpConfig / S3Config 同款）。
    #[test]
    fn the_debug_output_masks_the_presign_secret() {
        let env = env_map();
        let cfg = LocalStorageEnv::from_lookup(|k| env.get(k).cloned()).expect("应能构造");
        let debug = format!("{cfg:?}");
        assert!(
            !debug.contains("super-secret-value"),
            "Debug 不得含密钥：{debug}"
        );
        assert!(debug.contains("***"), "必须含打码标记：{debug}");
        // 阳性对照：证明 Debug 不是全空。
        assert!(
            debug.contains("/var/lib/base/storage"),
            "Debug 必须含根目录：{debug}"
        );
    }

    /// 配置源返回 `None` → 用默认值。
    #[tokio::test]
    async fn a_missing_config_falls_back_to_defaults() {
        struct Empty;
        impl StorageConfigSource for Empty {
            fn read_platform<'a>(
                &'a self,
                _ctx: &'a RequestContext,
                _key: &'a str,
            ) -> StorageBoxFuture<'a, Option<i64>> {
                Box::pin(async { None })
            }
        }
        let config = StorageConfig::new(Arc::new(Empty));
        let settings = config.settings(&RequestContext::new()).await;
        assert_eq!(settings, StorageSettings::default());
    }
}
