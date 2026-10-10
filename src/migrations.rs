use crate::{
    channel,
    engine::{Fault, Result},
};
use chrono::Utc;
use rusqlite::{params, Connection};

pub fn initialize(db: &mut Connection) -> Result<()> {
    let version: i64 = db.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version > 3 {
        return Err(Fault::new(
            503,
            "database_version_unsupported",
            "数据库来自更新的插件版本，请使用匹配版本或恢复备份",
        ));
    }
    db.execute_batch(include_str!("schema.sql"))?;
    if version < 2 {
        let tx = db.transaction()?;
        tx.execute_batch(
            "ALTER TABLE keys ADD COLUMN recording_since TEXT NOT NULL DEFAULT '';
             ALTER TABLE requests ADD COLUMN channel TEXT NOT NULL DEFAULT 'other';
             ALTER TABLE usage ADD COLUMN channel TEXT NOT NULL DEFAULT 'other';",
        )?;
        let since = Utc::now().to_rfc3339();
        tx.execute(
            "INSERT OR IGNORE INTO meta VALUES('continuous_recording_since',?1)",
            [&since],
        )?;
        tx.execute(
            "UPDATE keys SET recording_since=?1 WHERE recording_since=''",
            [&since],
        )?;
        for table in ["requests", "usage"] {
            let models = {
                let mut stmt = tx.prepare(&format!("SELECT DISTINCT model FROM {table}"))?;
                let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
                rows.collect::<std::result::Result<Vec<_>, _>>()?
            };
            for model in models {
                tx.execute(
                    &format!("UPDATE {table} SET channel=?1 WHERE model=?2"),
                    params![channel::classify(&model), model],
                )?;
            }
        }
        tx.execute_batch(
            "CREATE INDEX usage_channel_period ON usage(key_id,channel,started);
             CREATE INDEX requests_channel_review ON requests(key_id,channel,status);
             PRAGMA user_version=2;",
        )?;
        tx.commit()?;
    }
    if version < 3 {
        // v3 added the Gemini channel and short Kimi names. Only rows that were classified as
        // "other" by model name move; upstream-based channels such as Antigravity stay as recorded.
        let tx = db.transaction()?;
        for table in ["requests", "usage"] {
            let models = {
                let mut stmt = tx.prepare(&format!(
                    "SELECT DISTINCT model FROM {table} WHERE channel='other'"
                ))?;
                let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
                rows.collect::<std::result::Result<Vec<_>, _>>()?
            };
            for model in models {
                let channel = channel::classify(&model);
                if channel != "other" {
                    tx.execute(
                        &format!(
                            "UPDATE {table} SET channel=?1 WHERE model=?2 AND channel='other'"
                        ),
                        params![channel, model],
                    )?;
                }
            }
        }
        tx.execute_batch("PRAGMA user_version=3;")?;
        tx.commit()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::initialize;
    use rusqlite::Connection;

    #[test]
    fn v3_moves_only_name_based_other_rows() {
        let mut db = Connection::open_in_memory().unwrap();
        initialize(&mut db).unwrap();
        let row = |rid: &str, model: &str, channel: &str| {
            format!("INSERT INTO requests(request_id,model,key_id,requested,trace_id,started,status,channel)
                VALUES('{rid}','{model}','key','{model}','',0,'settled','{channel}');")
        };
        db.execute_batch(&format!(
            "{}{}{}{}PRAGMA user_version=2;",
            row("a", "gemini-3.6-flash-high", "other"),
            row("b", "k3", "other"),
            row("c", "gemini-3.6-flash-high", "antigravity"),
            row("d", "grok-4", "other"),
        ))
        .unwrap();
        initialize(&mut db).unwrap();
        let channel = |rid: &str| -> String {
            db.query_row(
                "SELECT channel FROM requests WHERE request_id=?1",
                [rid],
                |r| r.get(0),
            )
            .unwrap()
        };
        assert_eq!(channel("a"), "gemini");
        assert_eq!(channel("b"), "kimi");
        assert_eq!(channel("c"), "antigravity");
        assert_eq!(channel("d"), "other");
        let version: i64 = db
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(version, 3);
    }
}
