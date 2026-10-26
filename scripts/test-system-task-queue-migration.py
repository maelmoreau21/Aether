"""Exercise the populated SQLite queue migration with foreign keys enabled."""
from pathlib import Path
import sqlite3
import unittest


class SystemQueueMigration(unittest.TestCase):
    def test_preserves_tasks_dependencies_results_and_schedules(self):
        migrations = Path(__file__).resolve().parents[1] / "crates/core/migrations"
        with sqlite3.connect(":memory:") as connection:
            connection.execute("PRAGMA foreign_keys=ON")
            for name in ("0001_initial_schema.sql", "0002_automation.sql"):
                connection.executescript((migrations / name).read_text(encoding="utf-8"))
            connection.execute("INSERT OR IGNORE INTO media_nodes(id) VALUES('local')")
            connection.execute("INSERT INTO libraries(id,name,kind,node_id,root,enabled) VALUES('test','Test','films','local','/media',1)")
            for task in ("first", "second"):
                connection.execute("INSERT INTO durable_tasks(id,library_id,owner,idempotency_key,node_id,created_at,updated_at,available_at) VALUES(?,'test','test',?,'local',1,1,1)", (task, task))
            connection.execute("INSERT INTO automation_events(id,event_key,kind,library_id,asset_id,fingerprint,payload_json,created_at) VALUES('event','event','media_added','test','asset','fingerprint','{}',1)")
            connection.execute("INSERT INTO automation_task_links(task_id,event_id,extension_id,operation,fingerprint,generation,provider) VALUES('second','event','extension','metadata.propose','fingerprint','generation','provider')")
            connection.execute("INSERT INTO automation_dependencies VALUES('second','first')")
            connection.execute("INSERT INTO automation_results VALUES('extension','asset','fingerprint','second','{}',1)")
            connection.execute("INSERT OR IGNORE INTO system_task_schedules(id,name,description,category,interval_minutes,enabled,next_due_at) VALUES('clean-sessions','Clean','Description','Maintenance',60,1,100)")
            connection.commit()
            tables = ("durable_tasks", "automation_task_links", "automation_dependencies", "automation_results", "system_task_schedules")
            before = {table: connection.execute(f"SELECT * FROM {table}").fetchall() for table in tables}
            migration = (migrations / "0003_system_task_queue.sql").read_text(encoding="utf-8")
            connection.executescript("BEGIN;\n" + migration + "\nCOMMIT;")
            for table in tables:
                self.assertEqual(before[table], connection.execute(f"SELECT * FROM {table}").fetchall(), table)
            self.assertEqual([], connection.execute("PRAGMA foreign_key_check").fetchall())
            connection.execute("INSERT INTO durable_tasks(id,library_id,owner,idempotency_key,node_id,category,target_id,created_at,updated_at,available_at) VALUES('system',NULL,'system','system','local','system','clean-sessions',1,1,1)")
            connection.execute("UPDATE system_task_schedules SET last_outcome='queued'")
            with self.assertRaises(sqlite3.IntegrityError):
                connection.execute("INSERT INTO durable_tasks(id,library_id,owner,idempotency_key,node_id,created_at,updated_at,available_at) VALUES('invalid',NULL,'test','invalid','local',1,1,1)")


if __name__ == "__main__":
    unittest.main()
