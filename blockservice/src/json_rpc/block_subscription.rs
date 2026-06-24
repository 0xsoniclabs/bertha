// Copyright 2026 Sonic Operations Ltd
// This file is part of the Bertha testing infrastructure for Sonic.
//
// Bertha is free software: you can redistribute it and/or modify
// it under the terms of the GNU Lesser General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// Bertha is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Lesser General Public License for more details.
//
// You should have received a copy of the GNU Lesser General Public License
// along with Bertha. If not, see <http://www.gnu.org/licenses/>.

use std::time::Duration;

use bertha_types::Block;
use tokio::sync::mpsc::{self, Sender};
use tokio_stream::{Stream, wrappers::ReceiverStream};

use crate::json_rpc::{Error, Source};

/// Returns a stream of new blocks as they are added to the blockchain.
pub fn subscribe_to_blocks(
    start_block: u64,
    source: impl Source + 'static,
) -> impl Stream<Item = Result<Block, Error>> {
    const CHANNEL_SIZE: usize = 100;
    let (sender, receiver) = mpsc::channel(CHANNEL_SIZE);
    tokio::spawn(block_subscription_task(start_block, source, sender));
    ReceiverStream::new(receiver)
}

async fn block_subscription_task(
    start_block: u64,
    source: impl Source,
    sender: Sender<Result<Block, Error>>,
) {
    tokio::select! {
        _ = sender.closed() => (),
        r = async {
            let mut block_number = start_block;

            loop {
                let mut timeout = Duration::from_millis(10);
                let block_header_with_transactions_and_withdrawals = loop {
                    match source.get_block_header_with_transactions_and_withdrawals(block_number).await {
                        Ok(block_header_with_transactions_and_withdrawals) => break block_header_with_transactions_and_withdrawals,
                        Err(Error::NotFound) => {
                            // next block does not yet exist
                            tokio::time::sleep(timeout).await;
                            timeout *= 2;
                            continue
                        }
                        Err(err) => return Err(err),
                    }
                };

                let receipts = loop {
                    match source.get_block_receipts(block_number).await {
                        Ok(receipts) => break receipts,
                        Err(Error::NotFound) => {
                            // next block does not yet exist
                            tokio::time::sleep(timeout).await;
                            timeout *= 2;
                            continue
                        }
                        Err(err) => return Err(err),
                    }
                };

                let block = Block::from_parts(
                    block_header_with_transactions_and_withdrawals.block_header,
                    block_header_with_transactions_and_withdrawals.transactions,
                    receipts,
                    block_header_with_transactions_and_withdrawals.withdrawals,
                );

                // if receiver dropped
                if sender.send(Ok(block)).await.is_err() {
                    break;
                }
                block_number += 1;
            }
            Ok(())
        } => {
            if let Err(err) = r {
                // try to send the error; if this fails there is nothing we can do
                let _ = sender.send(Err(err)).await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use bertha_types::{BlockHeader, TransactionReceipt};
    use tokio_stream::StreamExt;

    use super::*;
    use crate::json_rpc::source::{BlockHeaderWithTransactionsAndWithdrawals, MockSource};

    #[tokio::test]
    async fn subscribe_to_blocks_sends_blocks_over_channel() {
        let start_block = 2;

        let mut mock_source = MockSource::new();
        mock_source
            .expect_get_block_header_with_transactions_and_withdrawals()
            //.withf(|number| ...) <- we can not constrain which blocks are requested because the subscription is running as a separate task and may produce more blocks than we consume in this test.
            .returning({
                let mut block_number = start_block;
                move |requested_block_number| {
                    assert_eq!(requested_block_number, block_number);
                    let curr_block_number = block_number;
                    block_number += 1;
                    Box::pin(async move {
                        Ok(BlockHeaderWithTransactionsAndWithdrawals {
                            block_header: BlockHeader {
                                number: curr_block_number,
                                ..BlockHeader::default()
                            },
                            transactions: Vec::new(),
                            withdrawals: Vec::new(),
                        })
                    })
                }
            });
        mock_source
            .expect_get_block_receipts()
            //.withf(|number| ...) <- we can not constrain which blocks are requested because the subscription is running as a separate task and may produce more blocks than we consume in this test.
            .returning({
                let mut block_number = start_block;
                move |requested_block_number| {
                    assert_eq!(requested_block_number, block_number);
                    block_number += 1;
                    Box::pin(async { Ok(vec![TransactionReceipt::default()]) })
                }
            });

        let mut stream = subscribe_to_blocks(start_block, mock_source);
        for i in start_block..=4 {
            let received_block = stream.next().await.unwrap();
            assert_eq!(received_block.unwrap().number, i);
        }
    }

    #[tokio::test]
    async fn subscribe_to_blocks_retries_when_block_header_and_receipt_do_not_exist_yet() {
        let mut mock_source = MockSource::new();
        mock_source
            .expect_get_block_header_with_transactions_and_withdrawals()
            .returning({
                let mut return_error = true;
                move |_| {
                    if return_error {
                        return_error = false;
                        Box::pin(async { Err(Error::NotFound) })
                    } else {
                        Box::pin(async { Ok(BlockHeaderWithTransactionsAndWithdrawals::default()) })
                    }
                }
            });
        mock_source.expect_get_block_receipts().returning({
            let mut return_error = true;
            move |_| {
                if return_error {
                    return_error = false;
                    Box::pin(async { Err(Error::NotFound) })
                } else {
                    Box::pin(async { Ok(vec![TransactionReceipt::default()]) })
                }
            }
        });

        let mut stream = subscribe_to_blocks(0, mock_source);
        let received_block = stream.next().await.unwrap();
        assert!(received_block.is_ok());
    }

    #[tokio::test]
    async fn subscribe_to_blocks_propagates_errors() {
        // get_block_header_with_transactions returns error
        {
            let mut mock_source = MockSource::new();
            mock_source
                .expect_get_block_header_with_transactions_and_withdrawals()
                .returning({
                    move |_| {
                        Box::pin(async move {
                            Err(Error::Serde(serde::de::Error::custom("some error")))
                        })
                    }
                });
            let mut stream = subscribe_to_blocks(0, mock_source);
            let received_block = stream.next().await.unwrap();
            assert!(received_block.is_err());
            assert!(
                received_block
                    .unwrap_err()
                    .to_string()
                    .contains("some error"),
            );
        }

        // get_block_receipt returns error
        {
            let mut mock_source = MockSource::new();
            mock_source
                .expect_get_block_header_with_transactions_and_withdrawals()
                .returning(|_| {
                    Box::pin(async { Ok(BlockHeaderWithTransactionsAndWithdrawals::default()) })
                });
            mock_source.expect_get_block_receipts().returning({
                move |_| {
                    Box::pin(async { Err(Error::Serde(serde::de::Error::custom("some error"))) })
                }
            });

            let mut stream = subscribe_to_blocks(0, mock_source);
            let received_block = stream.next().await.unwrap();
            assert!(received_block.is_err());
            assert!(
                received_block
                    .unwrap_err()
                    .to_string()
                    .contains("some error"),
            );
        }
    }

    #[tokio::test]
    async fn block_subscription_task_shuts_down_if_receiver_dropped() {
        let mut mock_source = MockSource::new();
        mock_source
            .expect_get_block_header_with_transactions_and_withdrawals()
            .returning(|_| {
                Box::pin(async { Ok(BlockHeaderWithTransactionsAndWithdrawals::default()) })
            });
        mock_source
            .expect_get_block_receipts()
            .returning(move |_| Box::pin(async { Ok(vec![TransactionReceipt::default()]) }));
        let (sender, receiver) = mpsc::channel(1);
        let task = tokio::spawn(block_subscription_task(0, mock_source, sender));
        tokio::task::yield_now().await;
        assert!(!task.is_finished());
        drop(receiver);
        tokio::task::yield_now().await;
        assert!(task.is_finished());
    }
}
