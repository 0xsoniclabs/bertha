use std::{path::Path, vec};

use crate::{
    cmd::make_progress_bar,
    db::{BLOCK_DB_NAME, BlockDb, RocksBlockDb},
    grpc::RpcClient,
};

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
    let from = from.unwrap_or_default();
    let to = to.unwrap_or(chain_id_remote_ranges.last().unwrap().to);

    if from > to {
        return Err("Invalid range: 'from' must be less than or equal to 'to'".into());
    }

    let local_ranges = db.get_ranges_of_chain_id(chain_id)?;
    let ranges_to_fetch = range_difference((from, to), local_ranges.as_slice());
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
    let progress_bar =
        make_progress_bar(ranges_to_fetch.iter().map(|r| r.1 - r.0 + 1).sum::<u64>())?;

    for (start, end) in ranges_to_fetch {
        // NOTE: We could spawn a task for each range to fetch blocks concurrently,
        // but for simplicity and to avoid overwhelming the server, we fetch them sequentially.
        let mut block_stream_response = client.get_block_range(chain_id, start, end).await?;
        // TODO:log each range being fetched to be able to write tests
        loop {
            let response = block_stream_response.message().await?;
            if response.is_none() {
                break; // No more blocks
            }
            let encoded_block = response.unwrap();

            uncompressed_bytes_written += encoded_block.data.len();
            db.put_raw(chain_id, encoded_block.number, &encoded_block.data)?;
            count += 1;
            progress_bar.inc(1);
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

/// Compute the difference between a minuend range and a set of subtrahend ranges.
fn range_difference(minuend: (u64, u64), subtrahend: &[(u64, u64)]) -> Vec<(u64, u64)> {
    let mut segments = vec![minuend];
    for range in subtrahend {
        segments = remove_range(&segments, *range);
    }
    segments
}

/// Remove the subtrahend range from the minuend segments.
fn remove_range(minuend: &Vec<(u64, u64)>, subtrahend: (u64, u64)) -> Vec<(u64, u64)> {
    let mut new_segments = vec![];
    for segment in minuend {
        match (segment.1.cmp(&subtrahend.0), segment.0.cmp(&subtrahend.1)) {
            // The segment is completely before or after the range
            (std::cmp::Ordering::Less, _) | (_, std::cmp::Ordering::Greater) => {
                new_segments.push(*segment);
            }
            // The segment overlaps with the range
            (_, _) => {
                if subtrahend.0 > segment.0 {
                    new_segments.push((segment.0, subtrahend.0 - 1));
                }
                if subtrahend.1 < segment.1 {
                    new_segments.push((subtrahend.1 + 1, segment.1));
                }
            }
        }
    }
    new_segments
}

#[cfg(test)]
mod tests {

    use std::path::Path;

    use prost::Message;
    use rand::{SeedableRng, rngs::SmallRng, seq::SliceRandom};

    use super::*;
    use crate::{
        cmd::{
            ChangeWorkingDir,
            fetch::{fetch, range_difference},
            init,
        },
        db::{BLOCK_DB_NAME, BlockDb, RocksBlockDb, proto},
        grpc::{
            proto_rpc::{BlockRange, BlockRangeRequest, ChainRange, ChainRanges, EncodedBlock},
            test_utils::{MockRpcServer, TestServer},
        },
    };

    #[test]
    fn range_difference_computes_correct_difference() {
        let cases = vec![
            // No local ranges, should return the whole range
            (vec![], (0, 30), vec![(0, 30)]),
            // Local ranges contiguous cover the whole range, should return empty
            (vec![(0, 30)], (0, 30), vec![]),
            // Local ranges do not cover the whole range, should return the missing parts
            (
                vec![(0, 5), (7, 10), (15, 20), (22, 28), (30, 30)],
                (0, 30),
                vec![(6, 6), (11, 14), (21, 21), (29, 29)],
            ),
            // Missing end of the range
            (
                vec![(0, 5), (7, 10), (15, 20)],
                (0, 30),
                vec![(6, 6), (11, 14), (21, 30)],
            ),
            // Missing start of the range
            (
                vec![(5, 10), (15, 20), (27, 30)],
                (0, 30),
                vec![(0, 4), (11, 14), (21, 26)],
            ),
            // Missing both ends of the range
            (
                vec![(5, 10), (15, 20), (27, 30)],
                (0, 30),
                vec![(0, 4), (11, 14), (21, 26)],
            ),
            // Difference is equal to a unit-size range (non-existing)
            (vec![(5, 5), (7, 10), (15, 20)], (5, 10), vec![(6, 6)]),
            // Difference is equal to a unit-size range (existing)
            (vec![(5, 5), (7, 10), (15, 20)], (15, 15), vec![]),
        ];

        let mut rng = SmallRng::seed_from_u64(123);
        for (local_ranges, requested_range, expected_difference) in cases {
            let intersection = range_difference(requested_range, &local_ranges);
            assert_eq!(
                intersection, expected_difference,
                "Failed for local_ranges: {local_ranges:?}, requested_range: {requested_range:?}"
            );
            // Randomize the order of local ranges
            let mut randomized_local_ranges = local_ranges.clone();
            randomized_local_ranges.shuffle(&mut rng);
            let intersection = range_difference(requested_range, &randomized_local_ranges);
            assert_eq!(
                intersection, expected_difference,
                "Failed for randomized local_ranges: {randomized_local_ranges:?}, requested_range: {requested_range:?}"
            );
        }
    }

    #[tokio::test]
    async fn fails_with_invalid_server() {
        // Invalid URL
        {
            let url = "invalid-url".to_string();
            let result = fetch(url, 1, None, None, std::io::sink()).await;
            let err = result.expect_err("fetch should fail with invalid url");

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
            let err = result.expect_err("fetch should fail when server not available");

            assert_eq!(err.to_string(), "transport error");
        }
    }

    #[tokio::test]
    async fn fails_with_db_error() {
        // Non-existing db
        {
            let tmpdir = tempfile::tempdir().unwrap();
            let _cwd = ChangeWorkingDir::new(tmpdir.path());
            let server = TestServer::new(MockRpcServer::new()).await;
            let result = fetch(server.address.clone(), 1, None, None, std::io::sink()).await;
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
            mock_server.expect_list().returning(|_| {
                Ok(tonic::Response::new(ChainRanges {
                    chain_ranges: vec![ChainRange {
                        chain_id: 1,
                        block_ranges: vec![BlockRange { from: 0, to: 10 }],
                    }],
                }))
            });
            let server = TestServer::new(mock_server).await;
            let result = fetch(server.address.clone(), 1, None, None, std::io::sink()).await;
            let err = result.expect_err("fetch should fail with invalid DB");
            assert!(err.to_string().contains("error in underlying storage"));
        }
    }

    #[tokio::test]
    async fn fails_with_server_error() {
        let tmpdir = tempfile::tempdir().unwrap();
        let _cwd = ChangeWorkingDir::new(tmpdir.path());
        init(None::<&Path>).unwrap();
        let mut mock_server = MockRpcServer::new();
        mock_server.expect_list().returning(|_| {
            Ok(tonic::Response::new(ChainRanges {
                chain_ranges: vec![ChainRange {
                    chain_id: 1,
                    block_ranges: vec![BlockRange { from: 0, to: 10 }],
                }],
            }))
        });
        mock_server
            .expect_get_block_range()
            .returning(|_| Err(tonic::Status::internal("Server error")));
        let server = TestServer::new(mock_server).await;

        let result = fetch(server.address.clone(), 1, None, None, std::io::sink()).await;

        let err = result.expect_err("fetch should fail with server error");
        assert!(err.to_string().contains("Server error"));
    }

    #[tokio::test]
    async fn fails_if_block_stream_response_contains_error() {
        let tmpdir = tempfile::tempdir().unwrap();
        let _cwd = ChangeWorkingDir::new(tmpdir.path());
        init(None::<&Path>).unwrap();
        let mut server = MockRpcServer::new();
        server.expect_list().returning(|_| {
            Ok(tonic::Response::new(ChainRanges {
                chain_ranges: vec![ChainRange {
                    chain_id: 1,
                    block_ranges: vec![BlockRange { from: 0, to: 10 }],
                }],
            }))
        });
        server.expect_get_block_range().returning(|_| {
            let error_response = vec![Err(tonic::Status::not_found("Block not found"))];
            Ok(tonic::Response::new(futures::stream::iter(error_response)))
        });

        let destructible_server = TestServer::new(server).await;
        let result = fetch(
            destructible_server.address.clone(),
            1,
            Some(0),
            Some(10),
            std::io::sink(),
        )
        .await;
        let err = result.expect_err("fetch should fail with block range error");
        assert!(err.to_string().contains("Block not found"));
    }

    #[tokio::test]
    async fn fails_with_no_remote_chain_ids() {
        let tmpdir = tempfile::tempdir().unwrap();
        let _cwd = ChangeWorkingDir::new(tmpdir.path());
        init(None::<&Path>).unwrap();
        let mut mock_server = MockRpcServer::new();
        mock_server.expect_list().returning(|_| {
            Ok(tonic::Response::new(ChainRanges {
                chain_ranges: vec![ChainRange {
                    chain_id: 2,
                    block_ranges: vec![BlockRange { from: 0, to: 10 }],
                }],
            }))
        });

        let destructible_server = TestServer::new(mock_server).await;
        let result = fetch(
            destructible_server.address.clone(),
            1, // Chain ID that does not exist
            None,
            None,
            std::io::sink(),
        )
        .await;
        let err = result.expect_err("fetch should fail with no remote chain IDs");
        assert!(err.to_string().contains("No ranges found for chain ID 1"));
    }

    #[tokio::test]
    async fn fails_with_invalid_range() {
        let tmpdir = tempfile::tempdir().unwrap();
        let _cwd = ChangeWorkingDir::new(tmpdir.path());
        init(None::<&Path>).unwrap();
        let mut mock_server = MockRpcServer::new();
        mock_server.expect_list().returning(|_| {
            Ok(tonic::Response::new(ChainRanges {
                chain_ranges: vec![ChainRange {
                    chain_id: 1,
                    block_ranges: vec![BlockRange { from: 0, to: 10 }],
                }],
            }))
        });

        let destructible_server = TestServer::new(mock_server).await;
        let result = fetch(
            destructible_server.address.clone(),
            1,
            Some(5),
            Some(3), // Invalid range (from > to)
            std::io::sink(),
        )
        .await;
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
        mock_server.expect_list().returning(|_| {
            Ok(tonic::Response::new(ChainRanges {
                chain_ranges: vec![ChainRange {
                    chain_id: 1,
                    block_ranges: vec![BlockRange { from: 0, to: 10 }],
                }],
            }))
        });

        let destructible_server = TestServer::new(mock_server).await;
        let result = fetch(
            destructible_server.address.clone(),
            1,
            Some(11), // Range that does not exist on the remote server
            Some(15),
            std::io::sink(),
        )
        .await;
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
        let mut local_blocks = vec![];
        for local_block_ranges in &local_blocks_ranges {
            for i in local_block_ranges.0..=local_block_ranges.1 {
                let b = bertha_types::Block {
                    number: i,
                    ..bertha_types::Block::default_sonic()
                };
                local_blocks.push(b.clone());
            }
        }
        let init_db = || {
            // Initialize the database
            let mut db = RocksBlockDb::open(db_path.clone()).unwrap();
            db.delete_range(1, None, None).unwrap(); // Clear the database
            assert!(db.get_ranges_of_chain_id(1).unwrap().is_empty()); // make sure the database is empty
            // Insert the local blocks into the database
            for block in local_blocks.clone() {
                db.put(1, block).unwrap();
            }
        };

        let remote_blocks_ranges_cases = vec![
            vec![(0, max_block_number)], // All blocks in one range
            vec![
                (0, 0),
                (3, 5),
                (10, 14),
                (20, 24),
                (30, 31),
                (33, 36),
                (38, 38),
            ], // Server has all blocks that are missing in client
        ];

        // The cases to test the fetch function
        // (from, to, expected ranges to fetch from the server, expected logging output)
        let fetch_cases = vec![
            // From and to are both None
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
            // from is None
            (
                None,
                Some(23),
                vec![(0, 0), (3, 5), (10, 14), (20, 23)],
                "Fetched and wrote 13 blocks, total uncompressed size: 0 MiB\n",
            ),
            // To is None
            (
                Some(1),
                None,
                vec![(3, 5), (10, 14), (20, 24), (30, 31), (33, 36), (38, 38)],
                "Fetched and wrote 20 blocks, total uncompressed size: 0 MiB\n",
            ),
            // From and to are both Some (Non-contiguous ranges)
            (
                Some(3),
                Some(23),
                vec![(3, 5), (10, 14), (20, 23)],
                "Fetched and wrote 12 blocks, total uncompressed size: 0 MiB\n",
            ),
            // From and to are both Some (Contiguous ranges)
            (
                Some(20),
                Some(24),
                vec![(20, 24)],
                "Fetched and wrote 5 blocks, total uncompressed size: 0 MiB\n",
            ),
            // From and to are both Some (locally available)
            (
                Some(15),
                Some(19),
                vec![],
                "No blocks to fetch for chain ID 1 in range 15 to 19\n",
            ),
        ];

        for remote_block_range in remote_blocks_ranges_cases {
            // Set what the server will return
            let list_response = ChainRanges {
                chain_ranges: vec![ChainRange {
                    chain_id: 1,
                    block_ranges: remote_block_range
                        .iter()
                        .map(|(from, to)| BlockRange {
                            from: *from,
                            to: *to,
                        })
                        .collect(),
                }],
            };
            for (from, to, expected_ranges_to_fetch, expected_output) in fetch_cases.clone() {
                init_db();
                let mut mock_server = MockRpcServer::new();
                mock_server.expect_list().returning({
                    let list_response = list_response.clone();
                    move |_| Ok(tonic::Response::new(list_response.clone()))
                });
                let mut sequence = mockall::Sequence::new();
                for (start, end) in &expected_ranges_to_fetch {
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
                    mock_server
                        .expect_get_block_range()
                        .times(1)
                        .in_sequence(&mut sequence)
                        .withf({
                            let start = *start;
                            let end = *end;
                            move |request: &tonic::Request<BlockRangeRequest>| {
                                let req = request.get_ref();
                                req.chain_id == 1 && req.from == start && req.to == end
                            }
                        })
                        .return_once(move |_| {
                            Ok(tonic::Response::new(futures::stream::iter(
                                range_response.clone(),
                            )))
                        });
                }
                let server = TestServer::new(mock_server).await;
                let mut buf = Vec::new();
                fetch(server.address.clone(), 1, from, to, &mut buf)
                    .await
                    .expect("Fetch should succeed");
                // Check that the output is as expected
                assert_eq!(String::from_utf8(buf).unwrap(), expected_output);
                // Check that the data were written to the database
                for (start, end) in expected_ranges_to_fetch {
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
