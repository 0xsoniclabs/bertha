use std::{fmt::Write, path::Path, vec};

use indicatif::{ProgressBar, ProgressState, ProgressStyle};

use crate::{
    blockdb::{BLOCK_DB_NAME, BlockDb, RocksBlockDb},
    rpc_client::RpcClient,
};

/// A function that computes the intersection between a set of ranges and a range
/// It returns the ranges that are included in range and not covered by the local ranges.
fn range_intersection(local_ranges: &[(u64, u64)], range: (u64, u64)) -> Vec<(u64, u64)> {
    let mut intersection: Vec<(u64, u64)> = vec![];

    let mut from = range.0;
    let to = range.1;
    let mut found = false;
    for (local_start, local_end) in local_ranges {
        // If the range to fetch is completely before the local range, we can add it directly
        if to < *local_start {
            intersection.push((from, to));
            found = true;
            break;
        }
        // range to fetch is completely after the local range, it may overlap with the next
        // local ranges
        if from > *local_end {
            continue;
        }
        // The range to fetch is completely within the local range, no need to fetch
        if from >= *local_start && to <= *local_end {
            found = true;
            break;
        }
        // Partial overlap, the range end could be included in a following local range
        if from >= *local_start && to > *local_end {
            from = local_end + 1;
        }
        // There is a gap between local and remote, we need to fetch the range
        if from < *local_start {
            intersection.push((from, *local_start - 1));
            // End of range is contained in the local range, we can stop here
            if to <= *local_end {
                found = true;
                break;
            }
            from = local_end + 1;
        }
    }
    // Range to fetch is completely after all local ranges
    if !found {
        intersection.push((from, to));
    }

    intersection
}

pub async fn fetch(
    url: String,
    chain_id: u64,
    from: Option<u64>,
    to: Option<u64>,
    mut writer: impl std::io::Write,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut client = RpcClient::try_new(url).await?;
    let mut uncompressed_bytes_written = 0;
    let db_path = Path::new("./").join(BLOCK_DB_NAME).canonicalize()?;
    let mut db = RocksBlockDb::open(db_path)?;

    // Get remote chain ranges
    let remote_ranges = client.list(Some(chain_id)).await?;
    let chain_id_remote_ranges = remote_ranges
        .chain_ranges
        .into_iter()
        .find(|r| r.chain_id == chain_id)
        .map(|r| r.block_ranges)
        .ok_or_else(|| format!("No ranges found for chain ID {chain_id}"))?;

    // Note: the chain id remote ranges are guaranteed to be non-empty
    let from = from.unwrap_or(chain_id_remote_ranges.first().unwrap().from);
    let to = to.unwrap_or(chain_id_remote_ranges.last().unwrap().to);

    if from > to {
        return Err("Invalid range: 'from' must be less than or equal to 'to'".into());
    }

    let local_ranges = db.get_ranges_of_chain_id(chain_id)?;
    let ranges_to_fetch = range_intersection(local_ranges.as_slice(), (from, to));
    if ranges_to_fetch.is_empty() {
        writeln!(
            writer,
            "No blocks to fetch for chain ID {chain_id} in range {from} to {to}"
        )?;
        return Ok(());
    }
    for range in &ranges_to_fetch {
        chain_id_remote_ranges
            .binary_search_by(|b| {
                if range.1 < b.from {
                    std::cmp::Ordering::Greater
                } else if range.0 > b.to {
                    std::cmp::Ordering::Less
                } else {
                    std::cmp::Ordering::Equal
                }
            })
            .map_err(|_| {
                format!(
                    "Range {} to {} for chain ID {chain_id} is not covered by remote ranges",
                    range.0, range.1
                )
            })?;
    }

    let mut count = 0;
    let progress_bar = ProgressBar::new(ranges_to_fetch.iter().map(|r| r.1 - r.0 + 1).sum::<u64>());
    progress_bar.set_style(
        ProgressStyle::with_template(
            "[{elapsed_precise}] [{wide_bar:.cyan/blue}] {pos}/{len} (ETA {eta})",
        )?
        .with_key("eta", |state: &ProgressState, w: &mut dyn Write| {
            // Since there is no way of propagating errors from this closure,
            // we just ignore the result (worst case the ETA will not be shown).
            let _ = write!(w, "{:.1}s", state.eta().as_secs_f64());
        })
        .progress_chars("#>-"),
    );

    for (start, end) in ranges_to_fetch {
        // NOTE: We could spawn a task for each range to fetch blocks concurrently,
        // but for simplicity and to avoid overwhelming the server, we fetch them sequentially.
        let mut blocks = client.get_block_range(chain_id, start, end).await?;

        loop {
            let response = blocks.message().await?;
            if response.is_none() {
                break; // No more blocks
            }
            let encoded_block = response.unwrap();

            uncompressed_bytes_written += encoded_block.data.len();
            db.put_raw(chain_id, encoded_block.number, &encoded_block.data)?;
            count += 1;
            progress_bar.set_position(count);
        }
        progress_bar.finish();
    }
    writeln!(
        writer,
        "Fetched and wrote {} blocks, total uncompressed size: {} MiB",
        count,
        uncompressed_bytes_written / (1024 * 1024)
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {

    use std::path::Path;

    use prost::Message;

    use super::*;
    use crate::{
        blockdb::{BLOCK_DB_NAME, BlockDb, RocksBlockDb},
        cmd::{
            ChangeWorkingDir,
            fetch::{fetch, range_intersection},
            init,
        },
        proto::{self},
        proto_rpc::{self, BlockRange, ChainRange, EncodedBlock},
        rpc_test_utils::{DestructibleServer, MockRpcServer},
    };

    #[test]
    fn range_difference_computes_correct_intersection() {
        // No local ranges
        let intersection = range_intersection(&[], (0, 30));
        assert_eq!(intersection, vec![(0, 30)]);

        // Local ranges cover the whole range
        let local_ranges = vec![(0, 30)];
        let intersection = range_intersection(&local_ranges, (0, 30));
        assert!(intersection.is_empty());

        // Local ranges do not cover the whole range
        let local_ranges = vec![(0, 5), (7, 10), (15, 20)];
        let intersection = range_intersection(&local_ranges, (0, 30));
        assert_eq!(intersection, vec![(6, 6), (11, 14), (21, 30)]);
    }

    #[tokio::test]
    async fn fails_with_invalid_server() {
        // Invalid URL
        {
            let url = "invalid-url".to_string();
            let result = fetch(url, 1, None, None, std::io::sink()).await;
            let err = result.expect_err("fetch should fail with non-existing DB");
            assert_eq!(err.to_string(), "transport error");
        }
        // Non-existent server
        {
            let result = fetch(
                "http://[::1]:9999".to_string(), // Assuming no server is running on this port
                1,
                None,
                None,
                std::io::sink(),
            )
            .await;
            let err = result.expect_err("fetch should fail with non-existing DB");
            assert_eq!(err.to_string(), "transport error");
        }
    }

    #[tokio::test]
    async fn fails_with_db_error() {
        // Non-existing db
        {
            let tmpdir = tempfile::tempdir().unwrap();
            let _cwd = ChangeWorkingDir::new(tmpdir.path());
            let mut server = DestructibleServer::new(MockRpcServer::new());
            server.wait_for_start().await;
            let result = fetch(server.get_url(), 1, None, None, std::io::sink()).await;
            server.shutdown();
            let err = result.expect_err("fetch should fail with non-existing DB");
            assert!(err.to_string().contains("No such file or directory"));
        }
        // Invalid stored chain ids
        {
            let tmpdir = tempfile::tempdir().unwrap();
            let _cwd = ChangeWorkingDir::new(tmpdir.path());
            init(None::<&Path>).unwrap();
            {
                let db_path = Path::new("./").join(BLOCK_DB_NAME).canonicalize().unwrap();
                let mut db = RocksBlockDb::open(db_path.clone()).unwrap();
                db.put_metadata_raw(1, vec![0].as_slice()).unwrap(); // Invalid metadata length
            }
            let mut mock_server = MockRpcServer::new();
            mock_server.list_response = Ok(proto_rpc::EncodedChainRanges {
                chain_ranges: vec![proto_rpc::ChainRange {
                    chain_id: 1,
                    block_ranges: vec![proto_rpc::BlockRange { from: 0, to: 10 }],
                }],
            });
            let mut server = DestructibleServer::new(mock_server);
            server.wait_for_start().await;
            let result = fetch(server.get_url(), 1, None, None, std::io::sink()).await;
            server.shutdown();
            let err = result.expect_err("fetch should fail with invalid DB");
            assert!(err.to_string().contains("error in underlying storage"));
        }
    }

    #[tokio::test]
    async fn fails_with_server_error() {
        let tmpdir = tempfile::tempdir().unwrap();
        let _cwd = ChangeWorkingDir::new(tmpdir.path());
        init(None::<&Path>).unwrap();
        let mut server = MockRpcServer::new();
        server.list_response = Ok(proto_rpc::EncodedChainRanges {
            chain_ranges: vec![proto_rpc::ChainRange {
                chain_id: 1,
                block_ranges: vec![proto_rpc::BlockRange { from: 0, to: 10 }],
            }],
        });
        server.get_block_range_response = Err(tonic::Status::internal("Server error"));
        let mut destructible_server = DestructibleServer::new(server);
        destructible_server.wait_for_start().await;

        let result = fetch(
            destructible_server.get_url(),
            1,
            None,
            None,
            std::io::sink(),
        )
        .await;

        destructible_server.shutdown();
        let err = result.expect_err("fetch should fail with server error");
        assert!(err.to_string().contains("Server error"));
    }

    #[tokio::test]
    async fn fails_if_block_range_contains_error() {
        let tmpdir = tempfile::tempdir().unwrap();
        let _cwd = ChangeWorkingDir::new(tmpdir.path());
        init(None::<&Path>).unwrap();
        let mut server = MockRpcServer::new();
        server.list_response = Ok(proto_rpc::EncodedChainRanges {
            chain_ranges: vec![proto_rpc::ChainRange {
                chain_id: 1,
                block_ranges: vec![proto_rpc::BlockRange { from: 0, to: 10 }],
            }],
        });
        server.get_block_range_response = Err(tonic::Status::not_found("Block not found"));
        let mut destructible_server = DestructibleServer::new(server);
        destructible_server.wait_for_start().await;

        let result = fetch(
            destructible_server.get_url(),
            1,
            Some(0),
            Some(10),
            std::io::sink(),
        )
        .await;

        destructible_server.shutdown();
        let err = result.expect_err("fetch should fail with block range error");
        assert!(err.to_string().contains("Block not found"));
    }

    #[tokio::test]
    async fn fails_with_no_remote_chain_ids() {
        let tmpdir = tempfile::tempdir().unwrap();
        let _cwd = ChangeWorkingDir::new(tmpdir.path());
        init(None::<&Path>).unwrap();
        let mut mock_server = MockRpcServer::new();
        mock_server.list_response = Ok(proto_rpc::EncodedChainRanges {
            chain_ranges: vec![ChainRange {
                chain_id: 2, // Different chain ID
                block_ranges: vec![BlockRange { from: 0, to: 10 }],
            }],
        });
        let mut destructible_server = DestructibleServer::new(mock_server);
        destructible_server.wait_for_start().await;
        let result = fetch(
            destructible_server.get_url(),
            1, // Chain ID that does not exist
            None,
            None,
            std::io::sink(),
        )
        .await;
        destructible_server.shutdown();
        let err = result.expect_err("fetch should fail with no remote chain IDs");
        assert!(err.to_string().contains("No ranges found for chain ID 1"));
    }

    #[tokio::test]
    async fn fails_with_invalid_range() {
        let tmpdir = tempfile::tempdir().unwrap();
        let _cwd = ChangeWorkingDir::new(tmpdir.path());
        init(None::<&Path>).unwrap();
        let mut mock_server = MockRpcServer::new();
        mock_server.list_response = Ok(proto_rpc::EncodedChainRanges {
            chain_ranges: vec![ChainRange {
                chain_id: 1,
                block_ranges: vec![BlockRange { from: 0, to: 10 }],
            }],
        });
        let mut destructible_server = DestructibleServer::new(mock_server);
        destructible_server.wait_for_start().await;
        let result = fetch(
            destructible_server.get_url(),
            1,
            Some(5),
            Some(3), // Invalid range (from > to)
            std::io::sink(),
        )
        .await;
        destructible_server.shutdown();
        let err = result.expect_err("fetch should fail with invalid range");
        assert_eq!(
            err.to_string(),
            "Invalid range: 'from' must be less than or equal to 'to'"
        );
    }

    #[tokio::test]
    async fn fails_with_missing_range_on_remote_server() {
        let tmpdir = tempfile::tempdir().unwrap();
        let _cwd = ChangeWorkingDir::new(tmpdir.path());
        init(None::<&Path>).unwrap();
        let mut mock_server = MockRpcServer::new();
        mock_server.list_response = Ok(proto_rpc::EncodedChainRanges {
            chain_ranges: vec![ChainRange {
                chain_id: 1,
                block_ranges: vec![BlockRange { from: 0, to: 10 }],
            }],
        });
        let mut destructible_server = DestructibleServer::new(mock_server);
        destructible_server.wait_for_start().await;
        let result = fetch(
            destructible_server.get_url(),
            1,
            Some(11), // Range that does not exist on the remote server
            Some(15),
            std::io::sink(),
        )
        .await;
        destructible_server.shutdown();
        let err = result.expect_err("fetch should fail with missing range on remote server");
        assert!(
            err.to_string()
                .contains("Range 11 to 15 for chain ID 1 is not covered by remote ranges")
        );
    }
    #[tokio::test]
    async fn retrieves_block_range_correctly() {
        let tmpdir = tempfile::tempdir().unwrap();
        let _cwd = ChangeWorkingDir::new(tmpdir.path());
        init(None::<&Path>).unwrap();
        let db_path = Path::new("./").join(BLOCK_DB_NAME).canonicalize().unwrap();

        let max_block_number: u64 = 40;
        let local_blocks_ranges = vec![
            (1, 2),
            (6, 9),
            (15, 19),
            (25, 29),
            (32, 32),
            (37, 37),
            (39, max_block_number),
        ];
        let mut blocks = vec![];
        for local_block_ranges in local_blocks_ranges {
            for i in local_block_ranges.0..=local_block_ranges.1 {
                let b = bertha_types::Block {
                    number: i,
                    ..bertha_types::Block::default_sonic()
                };
                blocks.push(b.clone());
            }
        }
        let init_db = {
            || {
                // Initialize the database
                let mut db = RocksBlockDb::open(db_path.clone()).unwrap();
                db.delete_range(1, None, None).unwrap(); // Clear the database
                assert!(db.get_ranges_of_chain_id(1).unwrap().is_empty()); // make sure the database is empty
                for block in blocks.clone() {
                    db.put(1, block).unwrap();
                }
            }
        };

        let remote_blocks_ranges_cases = vec![
            vec![(0, max_block_number)], /* All blocks in one
                                          * range */
            vec![
                (0, 0),
                (3, 5),
                (10, 14),
                (20, 24),
                (30, 31),
                (33, 36),
                (38, 38),
            ], // Server mirrors the client blocks
        ];

        let fetch_cases = vec![
            // From and to are both None, fetch all remote blocks
            (
                None,
                None,
                vec![
                    (0, 0),
                    (3, 5),
                    (10, 14),
                    (20, 24),
                    (30, 31),
                    (33, 36),
                    (38, 38),
                ],
                "Fetched and wrote 21 blocks, total uncompressed size: 0 MiB\n",
            ),
            // from not included, to not included
            (
                None,
                Some(23),
                vec![(0, 0), (3, 5), (10, 14), (20, 23)],
                "Fetched and wrote 13 blocks, total uncompressed size: 0 MiB\n",
            ),
            // from included, to not included
            (
                Some(1),
                None,
                vec![(3, 5), (10, 14), (20, 24), (30, 31), (33, 36), (38, 38)],
                "Fetched and wrote 20 blocks, total uncompressed size: 0 MiB\n",
            ),
            // from not included, to not included
            (
                Some(3),
                Some(5),
                vec![(3, 5)],
                "Fetched and wrote 3 blocks, total uncompressed size: 0 MiB\n",
            ),
            // from included, to not included
            (
                Some(1),
                Some(21),
                vec![(3, 5), (10, 14), (20, 21)],
                "Fetched and wrote 10 blocks, total uncompressed size: 0 MiB\n",
            ),
            // from not included, to included
            (
                Some(0),
                Some(17),
                vec![(0, 0), (3, 5), (10, 14)],
                "Fetched and wrote 9 blocks, total uncompressed size: 0 MiB\n",
            ),
            // from included, to included
            (
                Some(7),
                Some(28),
                vec![(10, 14), (20, 24)],
                "Fetched and wrote 10 blocks, total uncompressed size: 0 MiB\n",
            ),
            // from equals to to (non-existing)
            (
                Some(0),
                Some(0),
                vec![(0, 0)],
                "Fetched and wrote 1 blocks, total uncompressed size: 0 MiB\n",
            ),
            // from equals to to (existing)
            (
                Some(1),
                Some(1),
                vec![],
                "No blocks to fetch for chain ID 1 in range 1 to 1\n",
            ),
            // to equals to unit-size range
            (
                Some(16),
                Some(32),
                vec![(20, 24), (30, 31)],
                "Fetched and wrote 7 blocks, total uncompressed size: 0 MiB\n",
            ),
            // from equals to unit-size range
            (
                Some(32),
                Some(36),
                vec![(33, 36)],
                "Fetched and wrote 4 blocks, total uncompressed size: 0 MiB\n",
            ),
            // No blocks to fetch
            (
                Some(15),
                Some(19),
                vec![],
                "No blocks to fetch for chain ID 1 in range 15 to 19\n",
            ),
        ];

        for remote_block_range in remote_blocks_ranges_cases {
            let list_response = Ok(proto_rpc::EncodedChainRanges {
                chain_ranges: vec![proto_rpc::ChainRange {
                    chain_id: 1,
                    block_ranges: remote_block_range
                        .iter()
                        .map(|(from, to)| proto_rpc::BlockRange {
                            from: *from,
                            to: *to,
                        })
                        .collect(),
                }],
            });
            for (from, to, expected_ranges, expected_output) in fetch_cases.clone() {
                init_db();
                let mut encoded_blocks = vec![];
                for (start, end) in &expected_ranges {
                    let mut range_response: Vec<Result<EncodedBlock, tonic::Status>> = vec![];
                    for i in *start..=*end {
                        range_response.push(Ok(EncodedBlock {
                            number: i,
                            data: proto::Block::from(bertha_types::Block {
                                number: i,
                                ..bertha_types::Block::default_sonic()
                            })
                            .encode_to_vec(),
                        }));
                    }
                    encoded_blocks.push(range_response);
                }
                let mut mock_server = MockRpcServer::new();
                mock_server.list_response = list_response.clone();
                mock_server.get_block_range_response = Ok(encoded_blocks);
                let mut server = DestructibleServer::new(mock_server);
                server.wait_for_start().await;
                let mut buf = Vec::new();
                fetch(server.get_url(), 1, from, to, &mut buf)
                    .await
                    .expect("Fetch should succeed");
                server.shutdown();
                assert_eq!(String::from_utf8(buf).unwrap(), expected_output);
                // Check that the data was written to the database
                for (start, end) in expected_ranges {
                    for i in start..=end {
                        let db = RocksBlockDb::open_for_reading(db_path.clone()).unwrap();
                        let block = db.get(1, i).unwrap();
                        assert!(block.is_some(), "Block {i} not found in the database");
                        let block = block.unwrap();
                        assert_eq!(block.number, i);
                        assert_eq!(
                            block,
                            bertha_types::Block {
                                number: i,
                                ..bertha_types::Block::default_sonic()
                            }
                        );
                    }
                }
            }
        }
    }
}
