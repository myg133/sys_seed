//! 密码：Argon2id 哈希 + PHC 存储 + 本地常见口令词库。
//!
//! # 为什么是 Argon2id（design-summary §3）
//!
//! - 当前推荐（OWASP / NIST SP 800-63）：比 bcrypt 抗 GPU 更强，bcrypt 的 72 字节截断是已知缺陷；
//! - **参数可配**：低配开发机跑不动生产参数，生产值在部署配置里给足；
//! - **PHC 标准串存储**（`$argon2id$v=19$m=...,t=...,p=...$salt$hash`）：算法自描述，
//!   将来换算法或调参数**不用改表结构**。
//!
//! # 防账号枚举：假哈希校验（design-summary §5.1 / D6）
//!
//! 「账号不存在」如果直接返回，会比「密码错误」快得多 → **时序枚举**。
//! 所以账号不存在时也**执行一次哈希校验**（对固定的假哈希），让两条路径的耗时相当。
//! [`DummyHash`] 就是那个「固定假哈希」——它用**与真实密码相同的参数**生成，
//! 因此耗时也相同。

use std::sync::atomic::{AtomicU64, Ordering};

use argon2::password_hash::{PasswordHasher, PasswordVerifier, SaltString};
use argon2::{Algorithm, Argon2, Params, Version};
use rand::rngs::OsRng;

use crate::config::Argon2Settings;
use crate::error::IamError;

/// 本进程内 Argon2 **校验**的调用次数。
///
/// 存在的理由是可观测性：AC-S-021 要求「强制 SSO 时 Argon2id 调用次数 == 0」。
/// 「耗时同量级」没有区分力（三条路径都走同一段代码，即使真跑了 Argon2
/// 也仍然同量级），所以必须有一个**计数**可读的观测面。
///
/// 只计**校验**：登录路径上唯一会跑 Argon2 的地方就是它
/// （哈希只在注册 / 改密时发生，不在登录请求上）。因此这个计数从进程启动即为 0，
/// 「== 0」是**绝对值**而不是差值。
static ARGON2_INVOCATIONS: AtomicU64 = AtomicU64::new(0);

/// 读累计调用次数（进程级，只增不减）。
pub fn argon2_invocation_count() -> u64 {
    ARGON2_INVOCATIONS.load(Ordering::Relaxed)
}

/// 本地常见口令词库（本期只做本地小词库；完整 HIBP k-anonymity 接入放后续需求）。
///
/// 命中即拒绝注册 / 改密，错误码 `PASSWORD_POLICY_VIOLATION`。
const COMMON_PASSWORDS: [&str; 40] = [
    "123456",
    "123456789",
    "12345678",
    "1234567890",
    "password",
    "password1",
    "password123",
    "qwerty",
    "qwerty123",
    "abc123",
    "111111",
    "123123",
    "admin",
    "admin123",
    "administrator",
    "letmein",
    "welcome",
    "welcome1",
    "monkey",
    "dragon",
    "iloveyou",
    "sunshine",
    "princess",
    "football",
    "baseball",
    "trustno1",
    "changeme",
    "default",
    "secret",
    "root",
    "test",
    "password123456",
    "password1234",
    "qwertyuiop",
    "letmein123",
    "iloveyou123",
    "welcome123",
    "adminadmin",
    "passw0rd",
    "abc123456",
];

/// 构造 Argon2id 实例（按配置参数）。
fn argon2(settings: &Argon2Settings) -> Result<Argon2<'static>, IamError> {
    let params = Params::new(
        settings.memory_kib,
        settings.iterations,
        settings.parallelism,
        None,
    )
    .map_err(|err| IamError::Internal {
        reason: format!("the argon2 parameters are invalid: {err}"),
    })?;
    Ok(Argon2::new(Algorithm::Argon2id, Version::V0x13, params))
}

/// 用 Argon2id 哈希密码，返回 **PHC 标准串**。
///
/// 存储值**不含明文**，且算法与参数都写在串里（算法自描述）。
pub fn hash_password(password: &str, settings: &Argon2Settings) -> Result<String, IamError> {
    let hasher = argon2(settings)?;
    let salt = SaltString::generate(&mut OsRng);
    let hash = hasher
        .hash_password(password.as_bytes(), &salt)
        .map_err(|err| IamError::Internal {
            reason: format!("the password could not be hashed: {err}"),
        })?;
    Ok(hash.to_string())
}

/// 校验密码与 PHC 串是否匹配（**恒定时间比较**，由库实现）。
///
/// PHC 串本身非法时返回 `false`（不泄露「哈希格式错了」这种内部细节）。
pub fn verify_password(password: &str, phc: &str) -> bool {
    ARGON2_INVOCATIONS.fetch_add(1, Ordering::Relaxed);
    let Ok(parsed) = argon2::PasswordHash::new(phc) else {
        return false;
    };
    // 校验时必须用**存储串里的参数**（算法自描述的意义就在这里），
    // 而不是当前配置 —— 否则调过参数的老哈希全部失效。
    argon2::Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok()
}

/// 密码是否命中本地常见口令词库。
pub fn is_common_password(password: &str) -> bool {
    let lowered = password.trim().to_ascii_lowercase();
    COMMON_PASSWORDS
        .iter()
        .any(|candidate| lowered == *candidate)
        || COMMON_PASSWORDS
            .iter()
            .any(|candidate| lowered.starts_with(candidate) && lowered.len() <= candidate.len() + 2)
}

/// 「账号不存在」时用来对齐耗时的**固定假哈希**。
///
/// 生成一次，之后每次都复用（校验它与校验真哈希的耗时相同）。
#[derive(Debug, Clone)]
pub struct DummyHash {
    phc: String,
}

impl DummyHash {
    /// 用给定的参数生成假哈希。
    pub fn generate(settings: &Argon2Settings) -> Result<Self, IamError> {
        // 用一个固定的假口令：它永远不会被当成真密码（没人会知道它），
        // 但校验路径与真密码**完全一样**。
        let phc = hash_password(DUMMY_PLAINTEXT, settings)?;
        Ok(Self { phc })
    }

    /// 对假密码执行一次校验，让「账号不存在」的耗时与「密码错误」相当。
    pub fn verify(&self, password: &str) {
        let _ = verify_password(password, &self.phc);
    }

    /// 假哈希的 PHC 串（**只用于测试断言**：它绝不该出现在任何用户记录里）。
    pub fn phc(&self) -> &str {
        &self.phc
    }
}

/// 假哈希用的固定口令。
const DUMMY_PLAINTEXT: &str = "dummy-password-for-timing-equalization-only";

#[cfg(test)]
mod tests {
    use super::*;

    /// 测试用的**低**参数（低配机上不要用生产参数跑测试）。
    fn test_settings() -> Argon2Settings {
        Argon2Settings {
            memory_kib: 64,
            iterations: 1,
            parallelism: 1,
        }
    }

    #[test]
    fn the_stored_value_is_a_phc_string_and_hides_the_plaintext() {
        let phc =
            hash_password("correct horse battery staple", &test_settings()).expect("哈希应当成功");
        assert!(phc.starts_with("$argon2id$"), "phc = {phc}");
        assert!(!phc.contains("correct horse"), "PHC 串里不得含明文：{phc}");
        // 算法自描述：参数写在串里。
        assert!(phc.contains("m=64"), "phc = {phc}");
        assert!(phc.contains("t=1"), "phc = {phc}");
        assert!(phc.contains("p=1"), "phc = {phc}");
    }

    #[test]
    fn the_right_password_verifies_and_the_wrong_one_does_not() {
        let phc = hash_password("s3cret-passphrase", &test_settings()).expect("哈希应当成功");
        assert!(verify_password("s3cret-passphrase", &phc));
        assert!(!verify_password("s3cret-passphras3", &phc));
        assert!(!verify_password("", &phc));
    }

    /// AC-S-011：换参数不必改表结构（算法自描述）。
    #[test]
    fn a_hash_made_with_other_parameters_still_verifies() {
        let cheap = Argon2Settings {
            memory_kib: 64,
            iterations: 1,
            parallelism: 1,
        };
        let heavy = Argon2Settings {
            memory_kib: 128,
            iterations: 2,
            parallelism: 1,
        };
        let phc = hash_password("another-passphrase", &cheap).expect("哈希应当成功");
        assert!(phc.contains("m=64"));
        // 校验用的是**存储串里的参数**，所以参数变了也不影响已有哈希。
        assert!(verify_password("another-passphrase", &phc));
        let other = hash_password("another-passphrase", &heavy).expect("哈希应当成功");
        assert!(other.contains("m=128"));
        assert!(verify_password("another-passphrase", &other));
    }

    #[test]
    fn a_malformed_stored_hash_never_verifies() {
        assert!(!verify_password("anything", "not-a-phc-string"));
        assert!(!verify_password("anything", ""));
    }

    #[test]
    fn common_passwords_are_recognised() {
        assert!(is_common_password("password"));
        assert!(is_common_password("Password123"));
        assert!(is_common_password("  admin  "));
        assert!(!is_common_password("a-perfectly-fine-passphrase"));
    }

    /// 防枚举的关键：假哈希的校验路径与真哈希**完全一样**（同一套参数）。
    #[test]
    fn the_dummy_hash_uses_the_same_parameters() {
        let dummy = DummyHash::generate(&test_settings()).expect("假哈希应当生成");
        assert!(dummy.phc().starts_with("$argon2id$"));
        assert!(dummy.phc().contains("m=64"), "phc = {}", dummy.phc());
        // 假哈希**不是**别的口令的哈希（它只对应那个固定的假口令）。
        assert!(!verify_password("some-other-password", dummy.phc()));
        // 但校验它仍然会走完整的哈希路径（不报错、不短路）。
        dummy.verify("anything");
    }
}
