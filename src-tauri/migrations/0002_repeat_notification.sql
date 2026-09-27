-- 0002_repeat_notification（SPEC §5.2）：重复待办 + 提醒去重（P1，T3.4/T3.5）
-- 设计取舍：规则与主表分离，避免 todos 行变宽（对比 External 把规则列塞 todos 里）。

-- 重复规则（依附于模板 todo）
CREATE TABLE todo_repeat_rules (
  id         TEXT PRIMARY KEY,
  todo_id    TEXT NOT NULL REFERENCES todos(id) ON DELETE CASCADE,
  freq       TEXT NOT NULL CHECK (freq IN ('daily','weekly','monthly')),
  days_json  TEXT,                            -- weekly 时 [0-6]（0=周日）
  start_date INTEGER NOT NULL,                -- YYYYMMDD
  end_date   INTEGER,                         -- NULL=无限
  created_at INTEGER NOT NULL
);
CREATE INDEX idx_repeat_rules_todo ON todo_repeat_rules(todo_id);

-- 重复实例（每日生成；completed 独立于模板）
CREATE TABLE todo_repeat_items (
  id        TEXT PRIMARY KEY,
  todo_id   TEXT NOT NULL REFERENCES todos(id) ON DELETE CASCADE,
  date      INTEGER NOT NULL,                -- YYYYMMDD 实例日期
  completed INTEGER NOT NULL DEFAULT 0 CHECK (completed IN (0,1)),
  completed_at INTEGER,
  UNIQUE (todo_id, date)
);
CREATE INDEX idx_repeat_items_date ON todo_repeat_items(date);

-- 提醒去重（SPEC §13.1：同一待办同一天最多一条；External 踩过"疯狂弹通知"）
CREATE TABLE notification_log (
  todo_id  TEXT NOT NULL,
  date     INTEGER NOT NULL,                -- YYYYMMDD
  fired_at INTEGER NOT NULL,
  PRIMARY KEY (todo_id, date)
);
