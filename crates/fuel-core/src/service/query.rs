//! Queries we can run directly on `FuelService`.

use std::sync::Arc;

use fuel_core_types::{
    fuel_tx::{
        Transaction,
        UniqueIdentifier,
    },
    fuel_types::Bytes32,
    services::txpool::InsertionResult,
};
use futures::{
    Stream,
    StreamExt,
};

use crate::{
    query::transaction_status_change,
    schema::tx::types::TransactionStatus,
};

use super::*;

impl FuelService {
    /// Submit a transaction to the txpool.
    pub async fn submit(&self, tx: Transaction) -> anyhow::Result<InsertionResult> {
        let results: Vec<_> = self
            .shared
            .txpool
            .insert(vec![Arc::new(tx)])
            .await
            .into_iter()
            .collect::<Result<_, _>>()?;
        results
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("Nothing was inserted"))
    }

    /// Submit a transaction to the txpool and return a stream of status changes.
    pub async fn submit_and_status_change(
        &self,
        tx: Transaction,
    ) -> anyhow::Result<impl Stream<Item = anyhow::Result<TransactionStatus>>> {
        let id = tx.id(&self
            .shared
            .config
            .chain_conf
            .transaction_parameters
            .chain_id);
        let stream = self.transaction_status_change(id).await;
        self.submit(tx).await?;
        Ok(stream)
    }

    /// Submit a transaction to the txpool and return the final status.
    pub async fn submit_and_await_commit(
        &self,
        tx: Transaction,
    ) -> anyhow::Result<TransactionStatus> {
        let id = tx.id(&self
            .shared
            .config
            .chain_conf
            .transaction_parameters
            .chain_id);
        let stream = self.transaction_status_change(id).await.filter(|status| {
            futures::future::ready(!matches!(status, Ok(TransactionStatus::Submitted(_))))
        });
        futures::pin_mut!(stream);
        self.submit(tx).await?;
        stream
            .next()
            .await
            .ok_or_else(|| anyhow::anyhow!("Stream closed without transaction status"))?
    }

    /// Return a stream of status changes for a transaction.
    pub async fn transaction_status_change(
        &self,
        id: Bytes32,
    ) -> impl Stream<Item = anyhow::Result<TransactionStatus>> {
        let txpool = self.shared.txpool.clone();
        let db = self.shared.database.clone();
        let rx = Box::pin(txpool.tx_update_subscribe(id).await);
        transaction_status_change(
            move |id| match db.get_tx_status(&id)? {
                Some(status) => Ok(Some(status)),
                None => Ok(txpool.find_one(id).map(Into::into)),
            },
            rx,
            id,
        )
        .await
    }
}
