-- 基线种子数据
--
-- 幂等：可重复执行。`bun run seed` 会把它灌进 base_dev 与 base_test。
--
-- 本期平台尚无业务表（账号、组织等由 REQ-007/REQ-008 落地），
-- 因此这里只登记种子批次；后续模块的种子数据按批次追加。

INSERT INTO platform_seed_log (batch) VALUES ('baseline') ON CONFLICT (batch) DO NOTHING;
