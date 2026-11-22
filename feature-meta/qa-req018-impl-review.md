# REQ-018 实现阶段独立复核

> 复核者：独立 verifier 子 Agent（非实现者）
> 复核对象：feature/REQ-018 @ 待填
> 状态：进行中

## FINDING 清单

| # | 严重级 | 位置 | 问题 | 判据 |
|---|---|---|---|---|
| F-01 | **BLOCKER** | `crates/platform-storage/src/worker.rs:189-199` | 导入的**合法行从未写入目标表**：`Ok(()) => {}` 空分支，全仓无任何「entity → 目标表」的 INSERT 路径（`grep "INSERT INTO"` 只命中 object/upload/job/job_error 四张元数据表；`grep probe_rows` 只命中 `model.rs:531` 的单测字面量）。 | AC-S-180 步骤 5「目标表里**恰好 49990 行**（合法行全入库）」、AC-S-181 步骤 4「只有 3 行入库」——两条核心判据的实现不存在。这是 D325「每批一事务 insert_batch」的直接缺失。 |
| F-02 | **BLOCKER** | `crates/platform-storage/tests/storage_real_db.rs:339-444` | AC-S-180 的 5 万行判据被替换成 **2500 行**，且**完全没有断言目标表行数**（无 `SELECT count(*) FROM probe_rows`）。测试只断言 `processed_rows == 2500` / `failed_rows == 5` / 回执行号，即只验了「数了行」，没验「入了库」。 | F-01 因此**逃过测试**：把 AC-S-180 步骤 5（49990）与 AC-S-181 步骤 4（3 行入库）两条判据删掉后测试仍全绿。注释 `:336-338` 自认「规模差异在报告里如实说明」，但报告未说明，且判据不是「规模」问题而是「整条判据消失」。 |

## 复核范围与结论

（待填）
