-- 0003_fts：FTS5 全文搜索（F28/T4.5）
-- 选型注释：
-- - tokenizer='trigram'：支持中文任意子串（SQLite trigram 按每 3 字符滑窗建索引，
--   "周报" 命中标题里任意位置的片段；<3 字符 term 无 trigram，由查询侧 LIKE 兜底）；
-- - todos 用 FTS（正文+标题），id 列 UNINDEXED 做映射；
-- - memo/note 数据量小，查询侧直接 LIKE（note 全文进 FTS 若数据量涨再扩展，
--   当前 save_content 时同步 md 成本高，见 services/search.rs 注释）。

CREATE VIRTUAL TABLE todos_fts USING fts5(
  id UNINDEXED,
  title,
  content,
  tokenize='trigram'
);

-- 回填存量
INSERT INTO todos_fts (id, title, content) SELECT id, title, content FROM todos;