-- 0001_init：初始 schema（SPEC §5.1 定稿，逐字落地）
-- 一条迁移整体在一个事务内执行（db.rs runner 包裹）。
-- repeat/notification 相关表不在此建，等 0002（SPEC §5.2，禁止提前堆字段）。

CREATE TABLE schema_migrations (
  version    INTEGER PRIMARY KEY,
  description TEXT NOT NULL,
  applied_at INTEGER NOT NULL
);

CREATE TABLE settings (
  key        TEXT PRIMARY KEY,
  value      TEXT,
  updated_at INTEGER NOT NULL
);

-- 项目（继承旧版：工作任务归集）
CREATE TABLE projects (
  id          TEXT PRIMARY KEY,
  name        TEXT NOT NULL,
  description TEXT,
  color       TEXT,
  sort_order  INTEGER NOT NULL DEFAULT 0,
  archived    INTEGER NOT NULL DEFAULT 0 CHECK (archived IN (0,1)),
  created_at  INTEGER NOT NULL,
  updated_at  INTEGER NOT NULL
);

-- 人员（继承旧版：工作任务责任人/相关人，本地清单非账号体系）
CREATE TABLE people (
  id          TEXT PRIMARY KEY,
  name        TEXT NOT NULL,
  note        TEXT,
  sort_order  INTEGER NOT NULL DEFAULT 0,
  archived    INTEGER NOT NULL DEFAULT 0 CHECK (archived IN (0,1)),
  created_at  INTEGER NOT NULL,
  updated_at  INTEGER NOT NULL
);

-- 长期目标（继承旧版：目标跟踪）
CREATE TABLE goals (
  id          TEXT PRIMARY KEY,
  title       TEXT NOT NULL,
  category    TEXT,                             -- 工作/生活/成长 等
  description TEXT,
  status      TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active','done','archived')),
  sort_order  INTEGER NOT NULL DEFAULT 0,
  created_at  INTEGER NOT NULL,
  updated_at  INTEGER NOT NULL
);

CREATE TABLE todos (
  id           TEXT PRIMARY KEY,
  title        TEXT NOT NULL,
  content      TEXT,
  completed    INTEGER NOT NULL DEFAULT 0 CHECK (completed IN (0,1)),
  date         INTEGER,                         -- YYYYMMDD；NULL=收集箱
  sort_order   INTEGER NOT NULL DEFAULT 0,
  color        TEXT,                            -- red/orange/yellow/green/blue/purple/gray
  category     TEXT NOT NULL DEFAULT 'life' CHECK (category IN ('work','life')),  -- 继承旧版：工作/生活
  priority     TEXT NOT NULL DEFAULT 'medium' CHECK (priority IN ('high','medium','low')),  -- 继承旧版：高/中/低
  project_id   TEXT REFERENCES projects(id) ON DELETE SET NULL,
  time         TEXT,                            -- HH:MM 提醒时间，P1 启用
  parent_id    TEXT REFERENCES todos(id) ON DELETE CASCADE,  -- P1 子待办，0001 预留
  postpone_auto INTEGER NOT NULL DEFAULT 0 CHECK (postpone_auto IN (0,1)),
  created_at   INTEGER NOT NULL,
  updated_at   INTEGER NOT NULL,
  completed_at INTEGER
);
CREATE INDEX idx_todos_date     ON todos(date);
CREATE INDEX idx_todos_parent   ON todos(parent_id);
CREATE INDEX idx_todos_project  ON todos(project_id);
CREATE INDEX idx_todos_category ON todos(category);

-- 任务-人员分配（继承旧版 assignedTo / completedBy：多人分配，各自勾选）
CREATE TABLE todo_assignees (
  todo_id      TEXT NOT NULL REFERENCES todos(id) ON DELETE CASCADE,
  person_id    TEXT NOT NULL REFERENCES people(id) ON DELETE CASCADE,
  completed    INTEGER NOT NULL DEFAULT 0 CHECK (completed IN (0,1)),
  completed_at INTEGER,
  PRIMARY KEY (todo_id, person_id)
);
CREATE INDEX idx_todo_assignees_person ON todo_assignees(person_id);

-- 目标关联（待办/备忘/笔记都可挂目标；目标进度=关联待办完成率）
CREATE TABLE goal_links (
  goal_id      TEXT NOT NULL REFERENCES goals(id) ON DELETE CASCADE,
  item_type    TEXT NOT NULL CHECK (item_type IN ('todo','memo','note')),
  item_id      TEXT NOT NULL,
  created_at   INTEGER NOT NULL,
  PRIMARY KEY (goal_id, item_type, item_id)
);
CREATE INDEX idx_goal_links_item ON goal_links(item_type, item_id);

CREATE TABLE memos (
  id         TEXT PRIMARY KEY,
  title      TEXT NOT NULL,
  content    TEXT,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  archived   INTEGER NOT NULL DEFAULT 0 CHECK (archived IN (0,1))
);
CREATE INDEX idx_memos_archived ON memos(archived);

CREATE TABLE notes (
  id         TEXT PRIMARY KEY,
  title      TEXT NOT NULL,
  file_path  TEXT NOT NULL,                     -- 相对 data/ 的路径，如 notes/260915xxxxxxx.md
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  archived   INTEGER NOT NULL DEFAULT 0 CHECK (archived IN (0,1))
);

-- 三级实体 + 标签的通用关联
-- 修订（2026-09-16 T0.3，见 changelog）：SPEC §5.1 原表遗漏 to_id 列，但其下
-- idx_links_to(to_type, to_id) 与 F7 双向反链都需要它，故补上 to_id（语义=边的目标实体）。
CREATE TABLE links (
  id         TEXT PRIMARY KEY,
  from_type  TEXT NOT NULL CHECK (from_type IN ('todo','memo','note')),
  from_id    TEXT NOT NULL,
  to_type    TEXT NOT NULL CHECK (to_type   IN ('todo','memo','note')),
  to_id      TEXT NOT NULL,
  relation   TEXT NOT NULL CHECK (relation IN
             ('converted_to','derived_from','backlog','reference')),
  created_at INTEGER NOT NULL
);
CREATE INDEX idx_links_from ON links(from_type, from_id);
CREATE INDEX idx_links_to   ON links(to_type, to_id);

CREATE TABLE tags (
  id   TEXT PRIMARY KEY,
  name TEXT NOT NULL UNIQUE
);
CREATE TABLE item_tags (
  tag_id    TEXT NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
  item_type TEXT NOT NULL CHECK (item_type IN ('todo','memo','note')),
  item_id   TEXT NOT NULL,
  PRIMARY KEY (tag_id, item_type, item_id)
);
CREATE INDEX idx_item_tags_item ON item_tags(item_type, item_id);
