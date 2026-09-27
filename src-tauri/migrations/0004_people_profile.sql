-- 0004_people_profile：人员表补职责/邮箱独立列（原旧版导入把两者拼进 note，检索/编辑都不便）。
-- 两者均可空：历史数据与旧 JSON 缺失时不填；一条迁移整体在一个事务内执行（db.rs runner 包裹）。

ALTER TABLE people ADD COLUMN role TEXT;
ALTER TABLE people ADD COLUMN email TEXT;
