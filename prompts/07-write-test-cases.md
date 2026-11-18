# 任务：补测试用例

## 目标

给一段已有代码补测试用例，让「改错了能立刻被发现」。

## 输入

- `<模块名>`：业务模块名（若涉及前端则填 `packages/<包名>`）
- `<被测对象>`：函数 / 接口 / 组件（如 `repository::sort_rows`）
- `<要覆盖的场景>`：要覆盖的行为清单
- `<边界条件>`：空集合、重复主键、跨租户、超长输入等

## 步骤

1. **先读** `AGENTS.md` 与 `modules/<模块名>/AGENTS.md`，
   确认这个模块的约定（尤其是「易踩的坑」）。
2. **先看现有测试**：`cargo test -p <模块名> -- --test-threads=2 --list`
   列出已有用例，**不要重复写已经覆盖的场景**。
   模块的 `tests/contracts.rs` 已经覆盖了 C-1 ~ C-4
   （manifest 一致、权限点声明、租户隔离、事务边界）。
3. **分层补测**：
   - **单元测试**：放在被测代码同文件的 `#[cfg(test)] mod tests` 里
     （本仓库的惯例，见 `modules/sandbox/backend/src/repository.rs`）。
     纯函数优先用单元测试。
   - **集成 / 契约测试**：放在 `modules/<模块名>/tests/` 下。
     跨模块、跨层的行为放这里。
   - **前端测试**：`packages/<包名>/test/<name>.test.ts`（vitest）。
4. **按风险排序**：先补「错了会出事」的场景 ——
   租户隔离、主键重复、排序白名单外的字段、空集合、跨表写。
5. **断言要具体**：断言具体的错误信息 / 返回值，不要只断言 `is_err()`。
   本仓库的错误信息是稳定的英文串，可以直接断言。
6. **异步怎么等**：本模块不引异步运行时，仓库里已有极简 `block_on` 的写法
   （见 `modules/sandbox/backend/src/repository.rs` 的测试模块）——
   直接照抄，不要为此引入 `tokio`。
7. **自验**：
   ```bash
   cargo test -p <模块名> -- --test-threads=2
   cargo fmt --all --check
   cargo clippy --workspace --all-targets -- -D warnings
   ```

## 约束

- 不修改 `crates/platform-core/` 与 `crates/platform-contracts/`。
- 不为跑测试引入新的第三方依赖（不用 `mockall` / `proptest` / `tokio`）。
- 测试要能重复跑（不依赖执行顺序、不依赖真实时间 / 随机数 / 网络）。
- 不要写「只断言不抛异常」的空用例。

## 验收

- [ ] 新用例与已有用例不重复（列出对照）
- [ ] 覆盖了「边界条件」里列的每一项
- [ ] 断言具体到返回值 / 错误信息
- [ ] `cargo test -p <模块名> -- --test-threads=2` 全绿，用例数只增不减
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` 无告警
- [ ] 未新增任何第三方依赖
