-- 回滚 20261119000200_i18n_fx_rate.up.sql。
--
-- 本迁移**只**建了 `platform_fx_rate` 一张表（含索引与授权），
-- 所以回滚就是删这张表 —— 没有共享对象、没有改动既有表的列。
--
-- ⚠️ 删表会丢掉全部汇率快照。快照是**可追溯性**的证据（AC-S-097 步骤 4），
-- 生产环境回滚前应先导出（`COPY platform_fx_rate TO …`）。
--
-- `IF EXISTS` 让本脚本在「up 没跑成功」时也能安全执行（幂等）。

DROP TABLE IF EXISTS platform_fx_rate;
