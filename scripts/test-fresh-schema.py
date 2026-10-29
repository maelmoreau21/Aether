"""Validate the fresh SQLite section of the sole schema source."""

from pathlib import Path
import sqlite3
import unittest


SCHEMA = Path(__file__).resolve().parents[1] / "crates/core/schema.sql"
START = "-- AETHER_SCHEMA_SQLITE_BEGIN"
END = "-- AETHER_SCHEMA_SQLITE_END"


class FreshSchema(unittest.TestCase):
    def test_sqlite_tables_seeds_and_task_constraints(self):
        source = SCHEMA.read_text(encoding="utf-8")
        self.assertEqual(source.count(START), 1)
        self.assertEqual(source.count(END), 1)
        schema = source.split(START, 1)[1].split(END, 1)[0]
        with sqlite3.connect(":memory:") as connection:
            connection.execute("PRAGMA foreign_keys=ON")
            connection.executescript(schema)
            tables = {row[0] for row in connection.execute(
                "SELECT name FROM sqlite_master WHERE type='table'"
            )}
            self.assertTrue({
                "users", "libraries", "durable_tasks", "extension_objects",
                "automation_provider_cache", "automatic_timeline_markers",
            }.issubset(tables))
            self.assertNotIn("_sqlx_migrations", tables)
            self.assertEqual(connection.execute(
                "SELECT version FROM schema_version"
            ).fetchone()[0], 1)
            self.assertEqual(connection.execute(
                "SELECT COUNT(*) FROM media_nodes WHERE id='local'"
            ).fetchone()[0], 1)
            self.assertEqual(connection.execute(
                "SELECT COUNT(*) FROM system_task_schedules"
            ).fetchone()[0], 5)
            connection.execute(
                "INSERT INTO durable_tasks(id,owner,idempotency_key,node_id,category,target_id,created_at,updated_at,available_at) "
                "VALUES('system','system','system','local','system','clean-sessions',1,1,1)"
            )
            with self.assertRaises(sqlite3.IntegrityError):
                connection.execute(
                    "INSERT INTO durable_tasks(id,owner,idempotency_key,node_id,created_at,updated_at,available_at) "
                    "VALUES('invalid','user','invalid','local',1,1,1)"
                )
            connection.execute("INSERT INTO assets(sha256,size,mime,created_at) VALUES(?,1,'image/png',0)", ('a' * 64,))
            connection.execute("INSERT INTO avatar_packs(id,name,source) VALUES('native-pack','Pack','upload')")
            connection.execute("INSERT INTO avatar_images(id,pack_id,name,sha256) VALUES('image','native-pack','one',?)", ('a' * 64,))
            with self.assertRaises(sqlite3.IntegrityError):
                connection.execute("INSERT INTO avatar_images(id,pack_id,name,sha256) VALUES('bad','missing','one',?)", ('a' * 64,))
            connection.execute("DELETE FROM avatar_packs WHERE id='native-pack'")
            self.assertEqual(connection.execute("SELECT COUNT(*) FROM avatar_images").fetchone()[0], 0)
            connection.execute("INSERT INTO login_throttle(throttle_key,window_started,attempts) VALUES('timestamp-test',2147483648,1)")
            connection.execute("INSERT INTO users(id,role,created_at,disabled_at) VALUES('timestamp-test','user',2147483648,2147483648)")
            connection.execute("INSERT INTO api_keys(id,name,user_id,token_hash,created_at,expires_at,revoked_at,scope) VALUES('timestamp-key','test','timestamp-test','synthetic',2147483648,9999999999,2147483648,'catalog:read')")
            self.assertEqual(connection.execute("SELECT created_at,expires_at,revoked_at FROM api_keys WHERE id='timestamp-key'").fetchone(), (2147483648, 9999999999, 2147483648))
            self.assertEqual(connection.execute("PRAGMA foreign_key_check").fetchall(), [])


if __name__ == "__main__":
    unittest.main()
