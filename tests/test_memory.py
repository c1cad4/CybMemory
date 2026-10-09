import tempfile
from pathlib import Path
import unittest
from concurrent.futures import ThreadPoolExecutor
from cybmemory import Memory, Conflict
class MemoryTests(unittest.TestCase):
    def setUp(self):
        self.tmp=tempfile.TemporaryDirectory();self.addCleanup(self.tmp.cleanup)
        self.path=Path(self.tmp.name)/'knowledge.sqlite3';self.memory=Memory(self.path)
    def test_restart_and_unicode_search_preserve_provenance(self):
        with self.memory.connect() as db:self.memory.remember(db,'Пасека получает солнечную энергию','журнал пасеки','keeper')
        with Memory(self.path).connect() as db:
            rows=self.memory.search(db,'солнечную');self.assertEqual(rows[0]['source'],'журнал пасеки')
            self.assertEqual(rows[0]['agent_id'],'keeper')
    def test_failed_task_rolls_back_knowledge(self):
        def operation(db):
            self.memory.remember(db,'test','source','keeper');raise RuntimeError('abort')
        with self.assertRaises(RuntimeError):self.memory.execute_once('task',{},operation)
        with self.memory.connect() as db:self.assertEqual(db.execute('SELECT count(*) FROM knowledge').fetchone()[0],0)
    def test_concurrent_retries_write_once(self):
        def operation(db):return self.memory.remember(db,'remember once','source','keeper')
        with ThreadPoolExecutor(max_workers=6) as pool:rows=list(pool.map(lambda _:self.memory.execute_once('retry',{'content':'once'},operation),range(12)))
        self.assertEqual(len({r['id'] for r in rows}),1)
        with self.memory.connect() as db:self.assertEqual(db.execute('SELECT count(*) FROM knowledge').fetchone()[0],1)
    def test_conflicting_retry_rejected(self):
        self.memory.execute_once('retry',{'content':'first'},lambda db:{'ok':True})
        with self.assertRaises(Conflict):self.memory.execute_once('retry',{'content':'second'},lambda db:{'ok':False})
    def test_raw_fts_operators_are_literal(self):
        with self.memory.connect() as db:
            self.memory.remember(db,'apple banana','source','keeper')
            self.assertEqual(self.memory.search(db,'apple OR absent'),[])
