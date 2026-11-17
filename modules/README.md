# 业务模块落位目录
#
# 一个目录 = 一个完整业务模块（ADR-0001 D7）：
#   manifest.json / backend/ / frontend/ / migrations/ / AGENTS.md / README.md
#
# 目录规范由 `cargo xtask check-modules` 校验。
#
# 依赖方向恒为（平台代码永不反向依赖业务模块）：
#   platform-core (定义 trait) ← platform-contracts ← modules/<name>/backend
#
# 当前已有模块：
#   sandbox/  —— 示例模块，扩展点框架的标准范例（REQ-003）
