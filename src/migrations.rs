use crate::{
    channel,
    engine::{Fault, Result},
};
use chrono::Utc;
use rusqlite::{params, Connection};

pub fn initialize(db: &mut Connection) -> Result<()> {
    let version: i64 = db.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version > 2 {
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
    Ok(())
}
