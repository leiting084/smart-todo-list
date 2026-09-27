-- 0006_oplog（#32「最近操作」只读日志）：记录用户经 UI 的主要增删改，供回看"我刚才动了什么"。
-- 刻意只记不撤——撤销/回滚是另一套快照机制，风险高、收益低，本版不做。
-- 只在命令层（用户动作）写入，repo/importer/repeat 内部不写，避免批量导入与实例生成污染成海量噪声。
CREATE TABLE op_log (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  ts          INTEGER NOT NULL,          -- 毫秒时间戳
  action      TEXT NOT NULL,             -- create/update/delete/complete/uncomplete/archive/unarchive/import
  entity_type TEXT NOT NULL,             -- todo/project/memo/note/goal/person
  entity_id   TEXT,                      -- 目标 id（批量操作为 NULL）
  summary     TEXT NOT NULL              -- 人类可读摘要（标题/名称/批量计数）
);
CREATE INDEX idx_oplog_ts ON op_log(ts DESC);
