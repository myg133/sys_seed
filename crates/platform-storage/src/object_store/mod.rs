//! 对象存储抽象（REQ-018，D319）。
//!
//! # 两个实现
//!
//! | 实现 | 状态 |
//! |---|---|
//! | [`local_fs::LocalFsStore`] | ✅ **真实现**（真跑分片 / 断点续传 / 路径穿越防护） |
//! | [`s3::S3Store`] | ⚠️ **只做协议**（SigV4 签名按 AWS 官方向量验证，**未真连**）→ TD-059 |
//!
//! # ⚠️ 一条必须遵守的规则
//!
//! 「SigV4 签名对」**≠**「S3 能用」。单测能证明规范化请求 + HMAC 链正确，
//! 但**没有真实端点**证明 ① 我们的 REST 调用被 S3 接受 ② 分片合并的 ETag 处理正确。
//! → 报告里只许写「签名按官方向量验证通过」。

pub mod local_fs;
pub mod presign;
pub mod s3;

use std::future::Future;
use std::pin::Pin;

use bytes::Bytes;

use crate::error::StorageError;

/// 装箱的 future（让 trait 可对象化）。
pub type StoreFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, StorageError>> + Send + 'a>>;

/// 分片上传的会话 ID（不透明字符串）。
pub type UploadId = String;

/// 一个分片的 ETag。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartETag {
    /// 片号（1-based）。
    pub part_number: u32,
    /// 服务端给的 ETag（本地实现用 sha256 前 16 字节的 hex）。
    pub etag: String,
}

/// 对象存储的抽象。
///
/// ⚠️ 用 `StoreFuture` 而不是 `impl Future` —— 后者让 trait **不可对象化**
/// （REQ-015 的 `NotificationSender` 就是这个坑，见 D291 的说明）。
pub trait ObjectStore: Send + Sync {
    /// 写一个对象（覆盖）。
    fn put<'a>(&'a self, key: &'a str, bytes: Bytes) -> StoreFuture<'a, ()>;

    /// 读一个对象。
    fn get<'a>(&'a self, key: &'a str) -> StoreFuture<'a, Bytes>;

    /// 删一个对象（不存在视为成功 —— 幂等）。
    fn delete<'a>(&'a self, key: &'a str) -> StoreFuture<'a, ()>;

    /// 建分片上传会话。
    fn create_multipart<'a>(
        &'a self,
        upload_id: &'a str,
        key: &'a str,
        total_parts: i32,
    ) -> StoreFuture<'a, ()>;

    /// 上传一个分片（**幂等覆盖**）。
    fn upload_part<'a>(
        &'a self,
        upload_id: &'a str,
        part_number: u32,
        bytes: Bytes,
    ) -> StoreFuture<'a, PartETag>;

    /// 列出已上传的片号（**升序**）。
    fn list_parts<'a>(&'a self, upload_id: &'a str) -> StoreFuture<'a, Vec<u32>>;

    /// 合并分片（按片号顺序）并返回完整内容的 sha256。
    ///
    /// ⚠️ **必须按片号升序拼接**（不是按文件系统返回顺序）。
    fn complete_multipart<'a>(
        &'a self,
        upload_id: &'a str,
        key: &'a str,
        total_parts: i32,
    ) -> StoreFuture<'a, String>;

    /// 中止分片上传（删掉全部临时片）。
    fn abort_multipart<'a>(&'a self, upload_id: &'a str) -> StoreFuture<'a, ()>;
}

/// 计算 sha256（小写 hex）。
pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::Digest as _;
    let digest = sha2::Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// 校验对象 key 是否安全（**防路径穿越**，AC-S-175 步骤 7）。
///
/// 规则：
/// - 不能为空；
/// - 不能以 `/` 开头（绝对路径）；
/// - **任一段**不能是 `..` 或 `.`；
/// - 不能含 NUL 或反斜杠（Windows 路径分隔符）；
/// - 不能含控制字符。
pub fn is_safe_object_key(key: &str) -> bool {
    if key.is_empty() || key.starts_with('/') {
        return false;
    }
    if key.contains('\0') || key.contains('\\') {
        return false;
    }
    if key.chars().any(|c| c.is_control()) {
        return false;
    }
    key.split('/')
        .all(|segment| !segment.is_empty() && segment != ".." && segment != ".")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 安全的 key 通过。
    #[test]
    fn safe_keys_are_accepted() {
        for key in [
            "a.txt",
            "docs/a.txt",
            "a/b/c/d.bin",
            "含中文/文件.csv",
            "with space.txt",
        ] {
            assert!(is_safe_object_key(key), "应接受：{key}");
        }
    }

    /// 穿越 / 绝对路径 / 控制字符全部拒绝（TC-018-016 的基础）。
    #[test]
    fn unsafe_keys_are_rejected() {
        for key in [
            "",
            "/etc/passwd",
            "docs/../../etc/passwd",
            "../a",
            "a/../b",
            "docs/./a",
            "a\\b",
            "a\0b",
            "a\nb",
            "a//b",
        ] {
            assert!(!is_safe_object_key(key), "应拒绝：{key:?}");
        }
    }

    /// sha256 是小写 hex 且长度 64。
    #[test]
    fn sha256_is_lower_hex_of_length_64() {
        let digest = sha256_hex(b"hello");
        assert_eq!(digest.len(), 64);
        assert!(
            digest
                .chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
            "必须是**小写** hex：{digest}"
        );
        // 已知值（阳性对照 —— 证明算的是标准 sha256）。
        assert_eq!(
            digest,
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }

    /// 空内容的 sha256。
    #[test]
    fn the_empty_sha256_is_the_known_value() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }
}
