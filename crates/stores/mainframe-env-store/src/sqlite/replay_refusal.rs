//! Sole SQLite writer TX owns read/root fencing and refusal event/audit writes.
use super::*;
use mainframe_env_store_api::{CheckedReplayRefusalStep, ExecutionRecord};
#[cfg(test)]
#[path = "replay_refusal/tests.rs"]
mod tests;
impl SqliteStateStore {
    pub(crate) fn settle_checked_replay_refusal(
        &self,
        r: CheckedReplayRefusalStep,
    ) -> Result<ExecutionRecord, StoreError> {
        let mut budget = crate::replay_refusal::validate(&r, self.max_payload_bytes)?;
        block_on(&self.runtime, async {
            let mut tx = self.pool.begin().await.map_err(infrastructure)?;
            let outcome = async {
                self.check_reads_in(&mut tx,&r.effect,&r.execution,r.event.tick,&r.dependencies,
                    &mut budget).await?;
                let ns = format!("durable-event:{}",r.execution.execution_id);
                // One writer lock excludes phantoms. Bound/count the full keyset
                // BEFORE fetching keys or using it as cursor evidence.
                let (count,longest,bytes): (i64,i64,i64) = sqlx::query_as(
                    "SELECT COUNT(*),COALESCE(MAX(bytes),0),COALESCE(SUM(bytes),0) FROM (SELECT length(CAST(key AS BLOB)) AS bytes FROM provider_state WHERE namespace=? LIMIT 65537)")
                    .bind(&ns).fetch_one(&mut *tx).await.map_err(infrastructure)?;
                if count <= 0 || count as u64 != r.execution.version
                    || count as u64 > self.max_rows as u64 || count>65_536 || longest != 20 {
                    return Err(StoreError::InvalidSequence);
                }
                budget.add(usize::try_from(bytes).map_err(|_|StoreError::CapacityExceeded)?)?;
                let keys:Vec<String>=sqlx::query_scalar("SELECT key FROM provider_state WHERE namespace=? ORDER BY key LIMIT ?")
                    .bind(&ns).bind(count.checked_add(1).ok_or(StoreError::CapacityExceeded)?)
                    .fetch_all(&mut *tx).await.map_err(infrastructure)?;
                if keys.len()!=count as usize || keys.iter().enumerate().any(|(i,k)|
                    k!=&format!("{:020}",i+1)) {return Err(StoreError::InvalidSequence);}
                let last = self.checked_row_in(&mut tx,&ns,&format!("{:020}",r.execution.version),&mut budget)
                    .await?.ok_or(StoreError::NotFound)?;
                let last_event = crate::durable::decode_event(&last.payload)?;
                if last.version != 1 || last_event.execution_id != r.execution.execution_id
                    || last_event.run_unit_id != r.execution.run_unit_id
                    || last_event.attempt != r.execution.attempt
                    || last_event.sequence != r.execution.version || last_event.tick > r.event.tick {
                    return Err(StoreError::InvalidSequence);
                }
                // New sequence must be absent (including any forged future event).
                let maximum: String = sqlx::query_scalar("SELECT MAX(key) FROM provider_state WHERE namespace=?")
                    .bind(&ns).fetch_one(&mut *tx).await.map_err(infrastructure)?;
                if maximum != last.key { return Err(StoreError::InvalidSequence); }
                let (updated,mutations) = crate::replay_refusal::plan(&r,self.max_payload_bytes,&mut budget)?;
                self.apply_mutations_in(&mut tx,mutations).await?;
                Ok(updated)
            }.await;
            finish_read_transaction(tx, outcome).await
        })?
    }
}
