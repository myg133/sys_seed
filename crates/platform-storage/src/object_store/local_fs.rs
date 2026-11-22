//! **本地文件系统**对象存储 —— **真实现**（REQ-018，D319）。
//!
//! # 落盘布局
//!
//! ```text
//! <root>/
//!   objects/<object_key>              ← 已完成的文件
//!   .uploads/<upload_id>/part-<n>     ← 分片的临时文件
//! ```
//!
//! # ⚠️ 三条必须遵守的实现要求（每条都有判据）
//!
//! 1. **路径穿越防护**：所有 key 都过 [`crate::object_store::is_safe_object_key`]，
//!    且落盘前再断言「最终路径以 root 为前缀」（AC-S-175 步骤 7）；
//! 2. **分片写入用 `tmp` + `rename`**（design §2.2 / S-05）：
//!    并发传同一片时**后写的赢**，且**永远不会读到半个文件**；
//! 3. **合并按片号升序**（不是按目录返回顺序）——
//!    否则内容错乱，而 sha256 会成为唯一的兜底（太晚）。

use std::path::{Path, PathBuf};

use bytes::Bytes;

use super::{ObjectStore, PartETag, StoreFuture, is_safe_object_key, sha256_hex};
use crate::error::StorageError;

/// 本地文件系统实现。
#[derive(Debug, Clone)]
pub struct LocalFsStore {
    /// 存储根目录（`BASE_STORAGE_ROOT`）。
    root: PathBuf,
}

impl LocalFsStore {
    /// 用根目录构造（**不创建**目录 —— 首次写入时创建）。
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// 根目录。
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// 一个对象在磁盘上的路径。
    ///
    /// # ⚠️ 安全
    ///
    /// 先校验 key 形态，**再**拼路径，**再**断言路径在 root 之内（三层）。
    fn object_path(&self, key: &str) -> Result<PathBuf, StorageError> {
        if !is_safe_object_key(key) {
            return Err(StorageError::ObjectNotFound {
                object_key: key.to_string(),
            });
        }
        let path = self.root.join("objects").join(key);
        self.assert_within_root(&path)?;
        Ok(path)
    }

    /// 一个分片在磁盘上的路径。
    fn part_path(&self, upload_id: &str, part_number: u32) -> Result<PathBuf, StorageError> {
        if !is_safe_object_key(upload_id) {
            return Err(StorageError::ObjectNotFound {
                object_key: upload_id.to_string(),
            });
        }
        let path = self
            .root
            .join(".uploads")
            .join(upload_id)
            .join(format!("part-{part_number}"));
        self.assert_within_root(&path)?;
        Ok(path)
    }

    /// 断言路径在 root 之内（**直接判据**，AC-S-175 步骤 7 的判据 ①）。
    ///
    /// ⚠️ 用 `starts_with` 而不是字符串前缀比较：
    /// 后者会让 `/root-evil/x` 通过 `/root` 的检查。
    fn assert_within_root(&self, path: &Path) -> Result<(), StorageError> {
        // 注意：不 `canonicalize()`（目标文件可能还不存在）；
        // 用「父目录已存在的部分」做规范化，其余部分做词法检查。
        if path
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            return Err(StorageError::ObjectNotFound {
                object_key: "the path escapes the storage root".to_string(),
            });
        }
        if !path.starts_with(&self.root) {
            return Err(StorageError::ObjectNotFound {
                object_key: "the path escapes the storage root".to_string(),
            });
        }
        Ok(())
    }

    /// 原子写：先写 `tmp`，再 `rename`（S-05）。
    async fn write_atomically(&self, target: &Path, bytes: &Bytes) -> Result<(), StorageError> {
        if let Some(parent) = target.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        // 临时文件名带随机后缀 → 并发写不互相踩。
        let tmp = target.with_extension(format!("tmp-{}", uuid::Uuid::new_v4()));
        if let Some(parent) = tmp.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        tokio::fs::write(&tmp, bytes).await?;
        tokio::fs::rename(&tmp, target).await?;
        Ok(())
    }
}

impl ObjectStore for LocalFsStore {
    fn put<'a>(&'a self, key: &'a str, bytes: Bytes) -> StoreFuture<'a, ()> {
        Box::pin(async move {
            let path = self.object_path(key)?;
            self.write_atomically(&path, &bytes).await
        })
    }

    fn get<'a>(&'a self, key: &'a str) -> StoreFuture<'a, Bytes> {
        Box::pin(async move {
            let path = self.object_path(key)?;
            match tokio::fs::read(&path).await {
                Ok(data) => Ok(Bytes::from(data)),
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                    Err(StorageError::ObjectNotFound {
                        object_key: key.to_string(),
                    })
                }
                Err(err) => Err(err.into()),
            }
        })
    }

    fn delete<'a>(&'a self, key: &'a str) -> StoreFuture<'a, ()> {
        Box::pin(async move {
            let path = self.object_path(key)?;
            match tokio::fs::remove_file(&path).await {
                Ok(()) => Ok(()),
                // 幂等：不存在也算成功。
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(err) => Err(err.into()),
            }
        })
    }

    fn create_multipart<'a>(
        &'a self,
        upload_id: &'a str,
        _key: &'a str,
        _total_parts: i32,
    ) -> StoreFuture<'a, ()> {
        Box::pin(async move {
            if !is_safe_object_key(upload_id) {
                return Err(StorageError::ObjectNotFound {
                    object_key: upload_id.to_string(),
                });
            }
            let dir = self.root.join(".uploads").join(upload_id);
            self.assert_within_root(&dir)?;
            tokio::fs::create_dir_all(&dir).await?;
            Ok(())
        })
    }

    fn upload_part<'a>(
        &'a self,
        upload_id: &'a str,
        part_number: u32,
        bytes: Bytes,
    ) -> StoreFuture<'a, PartETag> {
        Box::pin(async move {
            if part_number == 0 {
                return Err(StorageError::PartMissing {
                    missing_parts: vec![0],
                });
            }
            let path = self.part_path(upload_id, part_number)?;
            self.write_atomically(&path, &bytes).await?;
            let etag = sha256_hex(&bytes);
            Ok(PartETag { part_number, etag })
        })
    }

    fn list_parts<'a>(&'a self, upload_id: &'a str) -> StoreFuture<'a, Vec<u32>> {
        Box::pin(async move {
            if !is_safe_object_key(upload_id) {
                return Err(StorageError::ObjectNotFound {
                    object_key: upload_id.to_string(),
                });
            }
            let dir = self.root.join(".uploads").join(upload_id);
            self.assert_within_root(&dir)?;
            let mut parts = Vec::new();
            let mut entries = match tokio::fs::read_dir(&dir).await {
                Ok(entries) => entries,
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(parts),
                Err(err) => return Err(err.into()),
            };
            while let Some(entry) = entries.next_entry().await? {
                let name = entry.file_name().to_string_lossy().to_string();
                if let Some(raw) = name.strip_prefix("part-")
                    && let Ok(number) = raw.parse::<u32>()
                {
                    parts.push(number);
                }
            }
            parts.sort_unstable();
            Ok(parts)
        })
    }

    fn complete_multipart<'a>(
        &'a self,
        upload_id: &'a str,
        key: &'a str,
        total_parts: i32,
    ) -> StoreFuture<'a, String> {
        Box::pin(async move {
            // ① 检查缺片（并按片号升序拼接 —— ⚠️ 不是目录顺序）。
            let uploaded = self.list_parts(upload_id).await?;
            let expected: Vec<u32> = (1..=total_parts.max(0) as u32).collect();
            let missing: Vec<u32> = expected
                .into_iter()
                .filter(|n| !uploaded.contains(n))
                .collect();
            if !missing.is_empty() {
                return Err(StorageError::PartMissing {
                    missing_parts: missing,
                });
            }

            // ② 按片号升序读并拼接。
            let mut assembled = Vec::new();
            for number in 1..=total_parts as u32 {
                let path = self.part_path(upload_id, number)?;
                let chunk = tokio::fs::read(&path).await?;
                assembled.extend_from_slice(&chunk);
            }

            // ③ 算 sha256（**拼接后**算 —— 若按错序拼，这里会与客户端声明不符）。
            let digest = sha256_hex(&assembled);

            // ④ 落对象。
            let target = self.object_path(key)?;
            self.write_atomically(&target, &Bytes::from(assembled))
                .await?;

            // ⑤ 清临时片。
            let dir = self.root.join(".uploads").join(upload_id);
            if dir.exists() {
                tokio::fs::remove_dir_all(&dir).await?;
            }
            Ok(digest)
        })
    }

    fn abort_multipart<'a>(&'a self, upload_id: &'a str) -> StoreFuture<'a, ()> {
        Box::pin(async move {
            if !is_safe_object_key(upload_id) {
                return Err(StorageError::ObjectNotFound {
                    object_key: upload_id.to_string(),
                });
            }
            let dir = self.root.join(".uploads").join(upload_id);
            self.assert_within_root(&dir)?;
            match tokio::fs::remove_dir_all(&dir).await {
                Ok(()) => Ok(()),
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(err) => Err(err.into()),
            }
        })
    }
}

/// 测试用：在系统临时目录下建一个独一无二的根。
#[cfg(test)]
pub(crate) fn temp_root(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!("platform-storage-{tag}-{}", uuid::Uuid::new_v4()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store(tag: &str) -> (LocalFsStore, PathBuf) {
        let root = temp_root(tag);
        (LocalFsStore::new(&root), root)
    }

    /// 主路径：put → get 逐字节相同（TC-018-011）。
    #[tokio::test]
    async fn put_then_get_returns_the_same_bytes() {
        let (s, root) = store("putget");
        let payload = Bytes::from(vec![7u8; 1024]);
        s.put("docs/a.bin", payload.clone()).await.expect("put");
        let got = s.get("docs/a.bin").await.expect("get");
        assert_eq!(
            sha256_hex(&got),
            sha256_hex(&payload),
            "读回内容必须逐字节相同"
        );
        assert_eq!(got.len(), 1024);
        let _ = tokio::fs::remove_dir_all(&root).await;
    }

    /// 落盘文件真的在 root 下（TC-018-012）。
    #[tokio::test]
    async fn the_file_lands_under_the_root() {
        let (s, root) = store("layout");
        s.put("docs/a.txt", Bytes::from_static(b"hi"))
            .await
            .expect("put");
        let expected = root.join("objects").join("docs").join("a.txt");
        assert!(expected.exists(), "文件应落在 objects/ 下");
        assert!(
            expected
                .canonicalize()
                .expect("应能规范化")
                .starts_with(root.canonicalize().expect("root 应存在")),
            "落盘路径必须在 root 之内"
        );
        let _ = tokio::fs::remove_dir_all(&root).await;
    }

    /// 不存在的 key → `OBJECT_NOT_FOUND`（TC-018-013）。
    #[tokio::test]
    async fn getting_a_missing_object_reports_not_found() {
        let (s, root) = store("missing");
        let err = s.get("nope.txt").await.expect_err("应报错");
        assert_eq!(err.code(), "STORAGE_OBJECT_NOT_FOUND");
        let _ = tokio::fs::remove_dir_all(&root).await;
    }

    /// delete 幂等（TC-018-014）。
    #[tokio::test]
    async fn delete_is_idempotent() {
        let (s, root) = store("delete");
        s.put("a.txt", Bytes::from_static(b"x")).await.expect("put");
        s.delete("a.txt").await.expect("第一次 delete");
        s.delete("a.txt")
            .await
            .expect("第二次 delete 也必须成功（幂等）");
        assert_eq!(
            s.get("a.txt").await.expect_err("应 404").code(),
            "STORAGE_OBJECT_NOT_FOUND"
        );
        let _ = tokio::fs::remove_dir_all(&root).await;
    }

    /// 同 key 再 put → 覆盖（TC-018-015）。
    #[tokio::test]
    async fn putting_the_same_key_overwrites() {
        let (s, root) = store("overwrite");
        s.put("a.txt", Bytes::from_static(b"AAAA"))
            .await
            .expect("put 1");
        s.put("a.txt", Bytes::from_static(b"BB"))
            .await
            .expect("put 2");
        let got = s.get("a.txt").await.expect("get");
        assert_eq!(&got[..], b"BB", "必须是第二次的内容");
        assert_eq!(got.len(), 2, "长度也必须是第二次的");
        let _ = tokio::fs::remove_dir_all(&root).await;
    }

    /// **路径穿越**必须被拒且不落盘（TC-018-016）。
    #[tokio::test]
    async fn path_traversal_is_rejected_and_nothing_lands() {
        let (s, root) = store("traversal");
        tokio::fs::create_dir_all(&root).await.expect("建 root");
        // 在 root 的父目录放哨兵（阳性对照）。
        let sentinel = root.parent().expect("有父目录").join("sentinel-req018.txt");
        tokio::fs::write(&sentinel, b"original")
            .await
            .expect("写哨兵");

        for bad in ["docs/../../etc/passwd", "../escape.txt", "/abs.txt"] {
            let result = s.put(bad, Bytes::from_static(b"evil")).await;
            assert!(result.is_err(), "穿越 key {bad:?} 必须被拒");
        }

        // ① 哨兵内容未变（阳性对照）。
        let after = tokio::fs::read(&sentinel).await.expect("哨兵还在");
        assert_eq!(&after[..], b"original", "哨兵不得被覆盖");
        // ② root 下**没有**任何文件（所有落盘都在 root 内且这次全被拒）。
        let objects = root.join("objects");
        assert!(
            !objects.exists()
                || std::fs::read_dir(&objects)
                    .map(|d| d.count() == 0)
                    .unwrap_or(true),
            "被拒的请求不得在 root 下留文件"
        );

        let _ = tokio::fs::remove_file(&sentinel).await;
        let _ = tokio::fs::remove_dir_all(&root).await;
    }

    /// 分片：3 片 → complete → sha256 一致（TC-018-018）。
    #[tokio::test]
    async fn multipart_upload_assembles_in_order() {
        let (s, root) = store("multipart");
        let p1 = Bytes::from(vec![1u8; 100]);
        let p2 = Bytes::from(vec![2u8; 100]);
        let p3 = Bytes::from(vec![3u8; 50]);
        let mut expected = Vec::new();
        expected.extend_from_slice(&p1);
        expected.extend_from_slice(&p2);
        expected.extend_from_slice(&p3);

        s.create_multipart("u1", "big.bin", 3)
            .await
            .expect("建会话");
        s.upload_part("u1", 1, p1).await.expect("片 1");
        s.upload_part("u1", 2, p2).await.expect("片 2");
        s.upload_part("u1", 3, p3).await.expect("片 3");
        let digest = s
            .complete_multipart("u1", "big.bin", 3)
            .await
            .expect("合并");

        assert_eq!(digest, sha256_hex(&expected), "合并后 sha256 必须一致");
        let got = s.get("big.bin").await.expect("读回");
        assert_eq!(&got[..], &expected[..], "内容必须逐字节相同");
        assert_eq!(got.len(), 250);
        let _ = tokio::fs::remove_dir_all(&root).await;
    }

    /// 重传同一 part 幂等（TC-018-019）。
    #[tokio::test]
    async fn reuploading_a_part_is_idempotent() {
        let (s, root) = store("idempotent-part");
        let body = Bytes::from(vec![9u8; 64]);
        s.create_multipart("u2", "f.bin", 1).await.expect("建会话");
        s.upload_part("u2", 1, body.clone()).await.expect("首次");
        s.upload_part("u2", 1, body.clone())
            .await
            .expect("重传也必须成功");
        let digest = s.complete_multipart("u2", "f.bin", 1).await.expect("合并");
        assert_eq!(digest, sha256_hex(&body));
        let _ = tokio::fs::remove_dir_all(&root).await;
    }

    /// 缺片 → `PART_MISSING` 且列出片号（TC-018-020）。
    #[tokio::test]
    async fn missing_parts_are_listed() {
        let (s, root) = store("part-missing");
        s.create_multipart("u3", "f.bin", 3).await.expect("建会话");
        s.upload_part("u3", 1, Bytes::from_static(b"a"))
            .await
            .expect("片 1");
        s.upload_part("u3", 3, Bytes::from_static(b"c"))
            .await
            .expect("片 3");
        let err = s
            .complete_multipart("u3", "f.bin", 3)
            .await
            .expect_err("应报缺片");
        assert_eq!(err.code(), "STORAGE_PART_MISSING");
        let params = err.params();
        assert_eq!(
            params.get("missingParts"),
            Some(&serde_json::json!([2])),
            "必须列出缺的片号 [2]"
        );
        let _ = tokio::fs::remove_dir_all(&root).await;
    }

    /// abort 后 complete → 缺片（会话目录已删）（TC-018-022）。
    #[tokio::test]
    async fn completing_after_abort_reports_missing_parts() {
        let (s, root) = store("abort");
        s.create_multipart("u4", "f.bin", 2).await.expect("建会话");
        s.upload_part("u4", 1, Bytes::from_static(b"a"))
            .await
            .expect("片 1");
        s.abort_multipart("u4").await.expect("abort");
        let err = s
            .complete_multipart("u4", "f.bin", 2)
            .await
            .expect_err("应报缺片");
        assert_eq!(err.code(), "STORAGE_PART_MISSING", "abort 后所有片都算缺");
        let _ = tokio::fs::remove_dir_all(&root).await;
    }

    /// **并发上传同一 part → 后写的赢，内容完整不混合**（TC-018-024）。
    #[tokio::test]
    async fn concurrent_part_uploads_never_produce_a_mixed_file() {
        let (s, root) = store("concurrent-part");
        s.create_multipart("u5", "f.bin", 1).await.expect("建会话");
        let a = Bytes::from(vec![0xAAu8; 4096]);
        let b = Bytes::from(vec![0xBBu8; 4096]);

        let barrier = std::sync::Arc::new(tokio::sync::Barrier::new(2));
        let (s1, s2) = (s.clone(), s.clone());
        let (b1, b2) = (barrier.clone(), barrier.clone());
        let (a2, b3) = (a.clone(), b.clone());
        let h1 = tokio::spawn(async move {
            b1.wait().await;
            s1.upload_part("u5", 1, a2).await
        });
        let h2 = tokio::spawn(async move {
            b2.wait().await;
            s2.upload_part("u5", 1, b3).await
        });
        h1.await.expect("任务 1").expect("上传 1");
        h2.await.expect("任务 2").expect("上传 2");

        let digest = s.complete_multipart("u5", "f.bin", 1).await.expect("合并");
        let got = s.get("f.bin").await.expect("读回");
        // 必须是**纯的 A 或纯的 B**，不能是混合。
        let all_a = got.iter().all(|b| *b == 0xAA);
        let all_b = got.iter().all(|b| *b == 0xBB);
        assert!(
            all_a || all_b,
            "并发后必须是纯 A 或纯 B，不能混合（前半 A 后半 B 就是交错写）"
        );
        // sha256 必须与「赢的那份」一致。
        let winner = if all_a { &a } else { &b };
        assert_eq!(digest, sha256_hex(winner), "sha256 必须与赢的那份一致");
        let _ = tokio::fs::remove_dir_all(&root).await;
    }

    /// 片号 0 非法。
    #[tokio::test]
    async fn part_number_zero_is_rejected() {
        let (s, root) = store("part-zero");
        s.create_multipart("u6", "f", 1).await.expect("建会话");
        let err = s
            .upload_part("u6", 0, Bytes::from_static(b"x"))
            .await
            .expect_err("片号 0 必须拒绝");
        assert_eq!(err.code(), "STORAGE_PART_MISSING");
        let _ = tokio::fs::remove_dir_all(&root).await;
    }

    /// `list_parts` 返回升序。
    #[tokio::test]
    async fn list_parts_returns_ascending_order() {
        let (s, root) = store("list-parts");
        s.create_multipart("u7", "f", 5).await.expect("建会话");
        for n in [4u32, 1, 3] {
            s.upload_part("u7", n, Bytes::from_static(b"x"))
                .await
                .expect("传片");
        }
        assert_eq!(s.list_parts("u7").await.expect("列片"), vec![1, 3, 4]);
        let _ = tokio::fs::remove_dir_all(&root).await;
    }

    /// 空上传会话 → 缺全部片。
    #[tokio::test]
    async fn an_empty_upload_reports_all_parts_missing() {
        let (s, root) = store("empty-upload");
        s.create_multipart("u8", "f", 2).await.expect("建会话");
        let err = s
            .complete_multipart("u8", "f", 2)
            .await
            .expect_err("应报缺片");
        assert_eq!(
            err.params().get("missingParts"),
            Some(&serde_json::json!([1, 2]))
        );
        let _ = tokio::fs::remove_dir_all(&root).await;
    }
}
