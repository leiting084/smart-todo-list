-- 0005 goal_profile：目标补充手动进度与目标日期（继承旧版 Electron 字段语义）。
-- 背景：目标与待办不一定关联，进度不应强制=关联待办完成率；
-- 旧版 ls-long-term-goals.json 的 progress(0-100)/targetDate(YYYY-MM-DD) 导入时曾被丢弃。
ALTER TABLE goals ADD COLUMN progress INTEGER NOT NULL DEFAULT 0; -- 手动进度 0-100
ALTER TABLE goals ADD COLUMN target_date INTEGER;                 -- 目标日期（毫秒时间戳）
