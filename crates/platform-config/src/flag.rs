//! Feature Flag 的名字规范与**确定性灰度**（D49 / D65 / 契约 C10 v2）。
//!
//! # 公式（写死，不能改）
//!
//! ```text
//! bucket  = sha256("<flag>:<user_id>") 的前 4 字节按大端解释为 u32
//! enabled = bucket % 100 < rolloutPercent
//! ```
//!
//! # 为什么不能用随机数 / `RandomState`
//!
//! | 方案 | 后果 |
//! |------|------|
//! | 每次请求掷骰子 | 同一用户在两次请求里得到不同结果 —— 界面一会儿有一会儿没有 |
//! | `std::collections::hash_map::RandomState` | **每个进程随机播种** → 多实例部署下同一用户在不同实例判出不同结果（表现为「一半请求命中」），比随机数更难排查 |
//!
//! 哈希 `(flag, user)` 得到**固定 bucket** 再与阈值比较，自然满足**单调性**：
//! 比例调高时已命中的用户不会掉出。反过来「每次重新分配」就会破坏它。
//!
//! # flag 名的规范化（D65）
//!
//! `^[a-z][a-zA-Z0-9]{0,63}$`：camelCase 起手、**无连字符**、最长 64。
//! **精确匹配、不做归一化**：`new-invoice` 与 `newInvoice` 是两个不同的 flag
//! （否则同名不同写法会互相串，灰度 bucket 也会完全不同）。
//! 非法名 → **400**，且**不进日志**（否则可伪造日志行 / 刷爆日志）。

use sha2::{Digest, Sha256};

/// 灰度比例允许的范围。
pub const ROLLOUT_RANGE: std::ops::RangeInclusive<i64> = 0..=100;
/// 灰度比例的默认值（只要总开关 `true` 就全量生效）。
pub const DEFAULT_ROLLOUT_PERCENT: i64 = 100;

/// flag 名是否合法：`^[a-z][a-zA-Z0-9]{0,63}$`。
pub fn is_valid_flag_name(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_ascii_lowercase() {
        return false;
    }
    let rest: Vec<char> = chars.collect();
    if rest.len() > 63 {
        return false;
    }
    rest.iter().all(|c| c.is_ascii_alphanumeric())
}

/// 一个用户对某个 flag 的灰度桶（`0..100` 之外的原始 `u32`）。
///
/// 输入用**登记名原文**：`sha256("<flag>:<user_id>")`。
pub fn bucket(flag: &str, user_id: &str) -> u32 {
    let mut hasher = Sha256::new();
    hasher.update(flag.as_bytes());
    hasher.update(b":");
    hasher.update(user_id.as_bytes());
    let digest = hasher.finalize();
    u32::from_be_bytes([digest[0], digest[1], digest[2], digest[3]])
}

/// 该用户是否落在灰度比例内。
///
/// `percent = 0` → 无人命中；`percent = 100` → 全部命中。
pub fn in_rollout(flag: &str, user_id: &str, percent: i64) -> bool {
    if percent <= 0 {
        return false;
    }
    if percent >= 100 {
        return true;
    }
    i64::from(bucket(flag, user_id) % 100) < percent
}

/// 日志里展示的 flag 名（**截断**，且不把控制字符原样写进日志行）。
pub fn display_name(name: &str) -> String {
    name.chars()
        .take(64)
        .map(|c| if c.is_control() { '?' } else { c })
        .collect()
}

/// 派生键：`flags.<flag>.rolloutPercent`（D57）。
pub fn rollout_key(flag: &str) -> String {
    format!("flags.{flag}.rolloutPercent")
}

/// flag 命名空间的前缀。
pub const FLAG_NAMESPACE_PREFIX: &str = "flags.";

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    /// 独立复算（**不复用**被测实现）：公式抄错也会被抓到。
    fn expected(flag: &str, user: &str, percent: i64) -> bool {
        let mut hasher = Sha256::new();
        hasher.update(format!("{flag}:{user}").as_bytes());
        let digest = hasher.finalize();
        let bucket = u32::from_be_bytes([digest[0], digest[1], digest[2], digest[3]]);
        (bucket % 100) < u32::try_from(percent).expect("比例应为 u32")
    }

    #[test]
    fn valid_flag_names_follow_the_camel_case_rule() {
        for name in ["beta", "newInvoice", "a", "aB0"] {
            assert!(is_valid_flag_name(name), "应接受：{name}");
        }
        for name in [
            "",
            "Beta",
            "new-invoice",
            "new_invoice",
            "0beta",
            "beta ",
            "bad\nINJECTED",
            &"a".repeat(65),
        ] {
            assert!(!is_valid_flag_name(name), "应拒绝：{name:?}");
        }
        assert!(is_valid_flag_name(&"a".repeat(64)), "64 个字符是允许的上界");
    }

    #[test]
    fn the_bucket_formula_matches_an_independent_computation() {
        for user in ["u-3001", "u-3002", "u-3177"] {
            for percent in [0, 1, 10, 50, 60, 99, 100] {
                assert_eq!(
                    in_rollout("beta", user, percent),
                    expected("beta", user, percent),
                    "user={user} percent={percent}"
                );
            }
        }
    }

    #[test]
    fn the_flag_name_is_part_of_the_hash_input() {
        // 同一用户在两个不同 flag 上的 bucket 不应被当成同一个「用户桶」。
        assert_ne!(bucket("beta", "u-3001"), bucket("newInvoice", "u-3001"));
    }

    #[test]
    fn boundaries_are_exact() {
        for user in ["u-3001", "u-3002", "u-3003"] {
            assert!(!in_rollout("beta", user, 0));
            assert!(in_rollout("beta", user, 100));
        }
    }

    #[test]
    fn monotonicity_holds_for_every_adjacent_threshold() {
        let users: Vec<String> = (0..500).map(|index| format!("u-{index}")).collect();
        let mut previous = 0usize;
        for percent in 0..=100 {
            let hits = users
                .iter()
                .filter(|user| in_rollout("beta", user, percent))
                .count();
            assert!(
                hits >= previous,
                "比例调高后命中数不得下降：percent={percent} hits={hits} previous={previous}"
            );
            previous = hits;
        }
        assert_eq!(previous, users.len(), "p=100 必须命中全部");
    }

    #[test]
    fn the_same_user_is_judged_identically_ten_times() {
        let first = in_rollout("beta", "u-3001", 50);
        for _ in 0..10 {
            assert_eq!(in_rollout("beta", "u-3001", 50), first);
        }
    }

    #[test]
    fn the_log_name_is_truncated_and_control_free() {
        let name = format!("bad\n{}", "a".repeat(200));
        let shown = display_name(&name);
        assert_eq!(shown.len(), 64);
        assert!(!shown.contains('\n'));
    }

    #[test]
    fn the_derived_key_is_the_documented_shape() {
        assert_eq!(rollout_key("beta"), "flags.beta.rolloutPercent");
    }
}
