use crate::StoreError;
use rusqlite::Connection;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProjectionMetadata {
    pub committed_revision: u64,
    pub committed_at_millis: i64,
}

pub(crate) fn read_projection_metadata(
    connection: &Connection,
) -> Result<ProjectionMetadata, StoreError> {
    let (committed_revision, committed_at_millis): (i64, i64) = connection
        .query_row(
            "SELECT committed_revision, committed_at FROM projection_metadata WHERE singleton_id = 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(StoreError::Sqlite)?;
    Ok(ProjectionMetadata {
        committed_revision: u64::try_from(committed_revision).map_err(|_| {
            StoreError::Contract("projection revision cannot be negative".into())
        })?,
        committed_at_millis,
    })
}
