"""SQLite knowledge with provenance and atomic idempotent local task execution."""
from contextlib import contextmanager
from pathlib import Path
import hashlib
import json
import sqlite3
import uuid

class Conflict(ValueError):
    pass

class Memory:
    def __init__(self, path):
        self.path = str(path)
        Path(path).parent.mkdir(parents=True, exist_ok=True)
        with self.connect() as db:
            db.execute('PRAGMA journal_mode=WAL')
            db.executescript("""
                CREATE TABLE IF NOT EXISTS knowledge(
                    id TEXT PRIMARY KEY, content TEXT NOT NULL, source TEXT NOT NULL,
                    agent_id TEXT NOT NULL, created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP);
                CREATE VIRTUAL TABLE IF NOT EXISTS knowledge_search USING fts5(
                    content, content='knowledge', content_rowid='rowid');
                CREATE TRIGGER IF NOT EXISTS knowledge_insert AFTER INSERT ON knowledge BEGIN
                    INSERT INTO knowledge_search(rowid, content) VALUES (new.rowid, new.content);
                END;
                CREATE TABLE IF NOT EXISTS tasks(
                    id TEXT PRIMARY KEY, digest TEXT NOT NULL, result TEXT NOT NULL,
                    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP);
            """)
    @contextmanager
    def connect(self):
        db = sqlite3.connect(self.path, timeout=10)
        db.row_factory = sqlite3.Row
        try:
            with db:
                yield db
        finally:
            db.close()
    def remember(self, db, content, source, agent_id):
        if not content.strip() or len(content) > 8000 or not source.strip() or len(source) > 256:
            raise ValueError('bounded content and provenance source required')
        identifier = str(uuid.uuid4())
        db.execute('INSERT INTO knowledge(id, content, source, agent_id) VALUES (?, ?, ?, ?)',
                   (identifier, content, source, agent_id))
        return dict(db.execute('SELECT * FROM knowledge WHERE id=?', (identifier,)).fetchone())
    def search(self, db, query):
        if not query.strip() or len(query) > 256:
            raise ValueError('query must contain 1–256 characters')
        # Treat terms as literal phrases rather than accepting raw FTS operators.
        terms = query.split()
        expression = ' AND '.join('"' + term.replace('"', '""') + '"' for term in terms)
        return [dict(r) for r in db.execute(
            'SELECT k.* FROM knowledge_search s JOIN knowledge k ON k.rowid=s.rowid '
            'WHERE knowledge_search MATCH ? ORDER BY rank LIMIT 20', (expression,))]
    def execute_once(self, task_id, payload, operation):
        if not task_id or len(task_id) > 128:
            raise ValueError('bounded task id required')
        digest = hashlib.sha256(json.dumps(payload, sort_keys=True, ensure_ascii=False).encode()).hexdigest()
        with self.connect() as db:
            db.execute('BEGIN IMMEDIATE')
            existing = db.execute('SELECT digest, result FROM tasks WHERE id=?', (task_id,)).fetchone()
            if existing:
                if existing['digest'] != digest:
                    raise Conflict('task id already used for a different request')
                return json.loads(existing['result'])
            result = operation(db)
            db.execute('INSERT INTO tasks(id, digest, result) VALUES (?, ?, ?)',
                       (task_id, digest, json.dumps(result, ensure_ascii=False)))
            return result
