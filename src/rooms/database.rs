//! Shared durability/resource policy for distinct SQLite room databases.
use crate::{Error, Result};
use rusqlite::{Connection, limits::Limit};
use std::time::Duration;

pub(super) fn configure(
    connection: &Connection,
    maximum_bytes: usize,
    maximum_value: usize,
) -> Result<()> {
    let value = i32::try_from(maximum_value)
        .map_err(|_| Error::Configuration("database value bound is too large"))?;
    connection
        .busy_timeout(Duration::from_millis(250))
        .map_err(|_| Error::Storage("cannot set lock deadline"))?;
    connection
        .set_limit(Limit::SQLITE_LIMIT_LENGTH, value)
        .map_err(|_| Error::Storage("cannot bound database values"))?;
    connection
        .execute_batch(
            "PRAGMA synchronous=FULL; PRAGMA journal_mode=DELETE; PRAGMA secure_delete=ON;
        PRAGMA foreign_keys=ON; PRAGMA trusted_schema=OFF; PRAGMA temp_store=MEMORY;",
        )
        .map_err(|_| Error::Storage("cannot enforce storage policy"))?;
    let page_size: usize = connection
        .pragma_query_value(None, "page_size", |row| row.get(0))
        .map_err(|_| Error::Storage("cannot determine database page size"))?;
    if !(512..=65_536).contains(&page_size) || !page_size.is_power_of_two() {
        return Err(Error::Storage("invalid database page size"));
    }
    connection
        .pragma_update(None, "max_page_count", maximum_bytes / page_size)
        .map_err(|_| Error::Storage("cannot bound database pages"))?;
    Ok(())
}
