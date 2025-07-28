use std::path::Path;

use crate::{
    BlockRange, app_dir::open_app_dir, cmd::make_progress_bar, db::BlockDb, grpc::RpcClient, utils,
};

/// Fetch a range of blocks for a specific chain ID from a remote server and store them in the local
/// database.
/// If `from` is not provided, it defaults to 0.
/// If `to` is not provided, it defaults to the last block of the chain on the remote server.
pub async fn fetch(
    app_dir: impl AsRef<Path>,
    url: String,
    chain_id: u64,
    from: Option<u64>,
    to: Option<u64>,
    mut writer: impl std::io::Write,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let (_cfg, db) = open_app_dir(app_dir, false)?;

    let mut client = RpcClient::try_new(url).await?;

    // Get remote chain ranges
    let remote_ranges = client.list(Some(chain_id)).await?;
    let chain_id_remote_ranges = remote_ranges
        .chain_ranges
        .into_iter()
        .find(|r| r.chain_id == chain_id)
        .map(|r| {
            r.block_ranges
                .into_iter()
                .map(BlockRange::from)
                .collect::<Vec<_>>()
        })
        .ok_or_else(|| format!("no ranges found for chain ID {chain_id}"))?;

    if chain_id_remote_ranges.is_empty() {
        return Err(
            format!("chain id {chain_id} found, but server returned empty chain range").into(),
        );
    }
    if !chain_id_remote_ranges.is_sorted_by_key(|range| (*range.start(), *range.end())) {
        return Err(format!("remote ranges for chain ID {chain_id} are not sorted").into());
    }

    let from = from.unwrap_or_default();
    let to = to.unwrap_or(*chain_id_remote_ranges.last().unwrap().end());

    if from > to {
        return Err("invalid range: 'from' must be less than or equal to 'to'".into());
    }

    let local_ranges = db.get_ranges_of_chain_id(chain_id)?;
    let ranges_to_fetch = utils::ranges::subtract_ranges(from..=to, local_ranges.as_slice());

    if ranges_to_fetch.is_empty() {
        writeln!(
            writer,
            "No blocks to fetch for chain ID {chain_id} in range {from} to {to}: All blocks are already available locally"
        )?;
        return Ok(());
    }

    // Check if the ranges to fetch are covered by the remote ranges by checking if each range is
    // contained within a remote range
    for range in &ranges_to_fetch {
        if chain_id_remote_ranges
            .binary_search_by(|remote_range| {
                if range.start() >= remote_range.start() && range.end() <= remote_range.end() {
                    std::cmp::Ordering::Equal
                } else if range.end() < remote_range.start() {
                    std::cmp::Ordering::Greater
                } else {
                    std::cmp::Ordering::Less
                }
            })
            .is_err()
        {
            return Err(format!(
                "range {} to {} for chain ID {chain_id} is not available on remote server",
                range.start(),
                range.end()
            )
            .into());
        }
    }

    let mut count = 0;
    let mut uncompressed_bytes_written = 0;
    let progress_bar = make_progress_bar(
        ranges_to_fetch
            .iter()
            .map(|r| *r.end() - *r.start() + 1)
            .sum::<u64>(),
    )?;

    for range in ranges_to_fetch {
        let from = *range.start();
        let to = *range.end();
        let mut expected_block = from;
        let mut block_range_stream_response = client.get_block_range(chain_id, from, to).await?;
        loop {
            let response = block_range_stream_response.message().await?;
            if response.is_none() {
                break; // No more blocks
            }
            let encoded_block = response.unwrap();
            if encoded_block.number != expected_block {
                return Err(format!(
                    "expected to receive block number {}, got {}",
                    expected_block, encoded_block.number
                )
                .into());
            }
            db.put_raw(chain_id, encoded_block.number, &encoded_block.data)?;
            uncompressed_bytes_written += encoded_block.data.len();
            expected_block += 1;
            progress_bar.inc(1);
        }
        if expected_block - 1 != to {
            return Err(format!(
                "received fewer blocks than expected: expected {}, got {}",
                to - from + 1,
                expected_block - from
            )
            .into());
        }
        count += (to - from + 1) as usize;
    }
    progress_bar.finish();

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
    use std::io::Cursor;

    use prost::Message;

    use crate::{
        BlockRange,
        app_dir::{init_app_dir, open_app_dir},
        cmd::{fetch::fetch, purge},
        db::{BlockDb, proto},
        grpc::{
            proto_rpc::{self, BlockRangeRequest, ChainRange, ChainRanges, EncodedBlock},
            test_utils::{MockRpcServer, TestServer},
        },
    };

    #[tokio::test]
    async fn fails_if_app_dir_is_not_initialized() {
        let tmpdir = tempfile::tempdir().unwrap();

        let server = TestServer::new(MockRpcServer::new()).await;
        let result = fetch(
            tmpdir.path(),
            server.address.clone(),
            1,
            None,
            None,
            std::io::sink(),
        )
        .await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains(&format!(
            "no blockservice.toml found at {} - did you forget to run init?",
            tmpdir.path().display()
        )));
    }

    #[tokio::test]
    async fn fails_for_invalid_server_url() {
        let tmpdir = tempfile::tempdir().unwrap();
        init_app_dir(tmpdir.path()).unwrap();

        let url = "invalid-url".to_string();
        let result = fetch(tmpdir.path(), url, 1, None, None, std::io::sink()).await;
        let err = result.expect_err("Fetch should fail with invalid url");

        assert_eq!(err.to_string(), "transport error");
    }

    #[tokio::test]
    async fn fails_on_invalid_stored_chain_ids() {
        let tmpdir = tempfile::tempdir().unwrap();
        init_app_dir(tmpdir.path()).unwrap();
        {
            let (_, db) = open_app_dir(tmpdir.path(), false).unwrap();
            db.put_metadata_raw(1, vec![0].as_slice()).unwrap(); // Invalid metadata length
        }
        let mut mock_server = MockRpcServer::new();
        mock_server.expect_list().returning(|_| {
            Ok(tonic::Response::new(ChainRanges {
                chain_ranges: vec![ChainRange {
                    chain_id: 1,
                    block_ranges: vec![proto_rpc::BlockRange { from: 0, to: 10 }],
                }],
            }))
        });
        let server = TestServer::new(mock_server).await;
        let result = fetch(
            tmpdir.path(),
            server.address.clone(),
            1,
            None,
            None,
            std::io::sink(),
        )
        .await;
        let err = result.expect_err("Fetch should fail with invalid DB");
        assert!(
            err.to_string()
                .contains("error in underlying storage layer: invalid ranges for chain ID 1")
        );
    }

    #[tokio::test]
    async fn fails_on_server_error() {
        let tmpdir = tempfile::tempdir().unwrap();
        init_app_dir(tmpdir.path()).unwrap();
        let mut mock_server = MockRpcServer::new();
        mock_server.expect_list().returning(|_| {
            Ok(tonic::Response::new(ChainRanges {
                chain_ranges: vec![ChainRange {
                    chain_id: 1,
                    block_ranges: vec![proto_rpc::BlockRange { from: 0, to: 10 }],
                }],
            }))
        });
        mock_server
            .expect_get_block_range()
            .returning(|_| Err(tonic::Status::internal("Server error")));
        let server = TestServer::new(mock_server).await;

        let result = fetch(
            tmpdir.path(),
            server.address.clone(),
            1,
            None,
            None,
            std::io::sink(),
        )
        .await;

        let err = result.expect_err("Fetch should fail with server error");
        assert!(err.to_string().contains("Server error"));
    }

    #[tokio::test]
    async fn fails_if_block_stream_response_contains_error() {
        let tmpdir = tempfile::tempdir().unwrap();
        init_app_dir(tmpdir.path()).unwrap();
        let mut server = MockRpcServer::new();
        server.expect_list().returning(|_| {
            Ok(tonic::Response::new(ChainRanges {
                chain_ranges: vec![ChainRange {
                    chain_id: 1,
                    block_ranges: vec![proto_rpc::BlockRange { from: 0, to: 10 }],
                }],
            }))
        });
        server.expect_get_block_range().returning(|_| {
            let error_response = vec![Err(tonic::Status::not_found("Block not found"))];
            Ok(tonic::Response::new(futures::stream::iter(error_response)))
        });

        let test_server = TestServer::new(server).await;
        let result = fetch(
            tmpdir.path(),
            test_server.address.clone(),
            1,
            Some(0),
            Some(10),
            std::io::sink(),
        )
        .await;
        let err = result.expect_err("Fetch should fail with block range error");
        assert!(err.to_string().contains("Block not found"));
    }

    #[tokio::test]
    async fn fails_if_no_remote_chain_ids() {
        let tmpdir = tempfile::tempdir().unwrap();
        init_app_dir(tmpdir.path()).unwrap();
        let mut mock_server = MockRpcServer::new();
        mock_server.expect_list().returning(|_| {
            Ok(tonic::Response::new(ChainRanges {
                chain_ranges: vec![ChainRange {
                    chain_id: 2,
                    block_ranges: vec![proto_rpc::BlockRange { from: 0, to: 10 }],
                }],
            }))
        });

        let test_server = TestServer::new(mock_server).await;
        let result = fetch(
            tmpdir.path(),
            test_server.address.clone(),
            1, // Chain ID that does not exist
            None,
            None,
            std::io::sink(),
        )
        .await;
        let err = result.expect_err("Fetch should fail with no remote chain IDs");
        assert_eq!(err.to_string(), "no ranges found for chain ID 1");
    }

    #[tokio::test]
    async fn fails_on_invalid_remote_chain_ranges() {
        let tmpdir = tempfile::tempdir().unwrap();
        init_app_dir(tmpdir.path()).unwrap();
        // Empty chain ranges for chain ID 1
        {
            let mut mock_server = MockRpcServer::new();
            mock_server.expect_list().returning(|_| {
                Ok(tonic::Response::new(ChainRanges {
                    chain_ranges: vec![ChainRange {
                        chain_id: 1,
                        block_ranges: vec![],
                    }],
                }))
            });
            let test_server = TestServer::new(mock_server).await;
            let result = fetch(
                tmpdir.path(),
                test_server.address.clone(),
                1, // Chain ID that exists but has no ranges
                None,
                None,
                std::io::sink(),
            )
            .await;
            let err = result.expect_err("Fetch should fail with empty chain ranges");
            assert_eq!(
                err.to_string(),
                "chain id 1 found, but server returned empty chain range"
            );
        }
        // Non-sorted chain ranges for chain ID 1
        {
            let mut mock_server = MockRpcServer::new();
            mock_server.expect_list().returning(|_| {
                Ok(tonic::Response::new(ChainRanges {
                    chain_ranges: vec![ChainRange {
                        chain_id: 1,
                        block_ranges: to_proto_block_range([10..=20, 0..=5]), // Not sorted
                    }],
                }))
            });
            let test_server = TestServer::new(mock_server).await;
            let result = fetch(
                tmpdir.path(),
                test_server.address.clone(),
                1, // Chain ID that exists but has unsorted ranges
                None,
                None,
                std::io::sink(),
            )
            .await;
            let err = result.expect_err("Fetch should fail with unsorted chain ranges");
            assert_eq!(
                err.to_string(),
                "remote ranges for chain ID 1 are not sorted"
            );
        }
    }

    #[tokio::test]
    async fn fails_on_invalid_requested_range() {
        let tmpdir = tempfile::tempdir().unwrap();
        init_app_dir(tmpdir.path()).unwrap();
        let mut mock_server = MockRpcServer::new();
        mock_server.expect_list().returning(|_| {
            Ok(tonic::Response::new(ChainRanges {
                chain_ranges: vec![ChainRange {
                    chain_id: 1,
                    block_ranges: vec![proto_rpc::BlockRange { from: 0, to: 10 }],
                }],
            }))
        });

        let test_server = TestServer::new(mock_server).await;
        let result = fetch(
            tmpdir.path(),
            test_server.address.clone(),
            1,
            Some(5),
            Some(3), // Invalid range (from > to)
            std::io::sink(),
        )
        .await;
        let err = result.expect_err("Fetch should fail with invalid range");
        assert_eq!(
            err.to_string(),
            "invalid range: 'from' must be less than or equal to 'to'"
        );
    }

    #[tokio::test]
    async fn fails_if_requested_range_does_not_exist_on_remote_server() {
        let tmpdir = tempfile::tempdir().unwrap();
        init_app_dir(tmpdir.path()).unwrap();
        let mut mock_server = MockRpcServer::new();
        mock_server.expect_list().returning(|_| {
            Ok(tonic::Response::new(ChainRanges {
                chain_ranges: vec![ChainRange {
                    chain_id: 1,
                    block_ranges: vec![proto_rpc::BlockRange { from: 5, to: 10 }],
                }],
            }))
        });

        let test_server = TestServer::new(mock_server).await;
        // Range is completely after the remote ranges
        {
            let result = fetch(
                tmpdir.path(),
                test_server.address.clone(),
                1,
                Some(11), // Range that does not exist on the remote server
                Some(15),
                std::io::sink(),
            )
            .await;
            let err = result.expect_err("Fetch should fail with missing range on remote server");
            assert_eq!(
                err.to_string(),
                "range 11 to 15 for chain ID 1 is not available on remote server"
            );
        }
        // Range is completely before the remote ranges
        {
            let result = fetch(
                tmpdir.path(),
                test_server.address.clone(),
                1,
                Some(0),
                Some(4), // Range that exists locally but not on the remote server
                std::io::sink(),
            )
            .await;
            let err = result.expect_err("Fetch should fail with missing range on remote server");
            assert_eq!(
                err.to_string(),
                "range 0 to 4 for chain ID 1 is not available on remote server"
            );
        }
        // Range partially overlaps with the remote ranges but is not fully covered
        {
            let result = fetch(
                tmpdir.path(),
                test_server.address.clone(),
                1,
                Some(5),
                Some(15), // Range that partially overlaps with the remote ranges
                std::io::sink(),
            )
            .await;
            let err = result.expect_err("Fetch should fail with missing range on remote server");
            assert_eq!(
                err.to_string(),
                "range 5 to 15 for chain ID 1 is not available on remote server"
            );
        }
    }

    #[tokio::test]
    async fn fails_if_get_block_range_returns_unexpected_block_range() {
        let tmpdir = tempfile::tempdir().unwrap();
        init_app_dir(tmpdir.path()).unwrap();
        // Less block than expected
        {
            let mut mock_server = MockRpcServer::new();
            mock_server.expect_list().returning(|_| {
                Ok(tonic::Response::new(ChainRanges {
                    chain_ranges: vec![ChainRange {
                        chain_id: 1,
                        block_ranges: vec![proto_rpc::BlockRange { from: 0, to: 10 }],
                    }],
                }))
            });

            // Simulate a response that returns fewer blocks than requested
            mock_server.expect_get_block_range().returning(|_| {
                let response = vec![
                    Ok(EncodedBlock {
                        number: 0,
                        data: proto::Block::from(bertha_types::Block {
                            number: 0,
                            ..bertha_types::Block::default_sonic()
                        })
                        .encode_to_vec(),
                    }),
                    Ok(EncodedBlock {
                        number: 1,
                        data: proto::Block::from(bertha_types::Block {
                            number: 1,
                            ..bertha_types::Block::default_sonic()
                        })
                        .encode_to_vec(),
                    }),
                ]; // Only two blocks instead of three
                Ok(tonic::Response::new(futures::stream::iter(response)))
            });

            let test_server = TestServer::new(mock_server).await;
            let result = fetch(
                tmpdir.path(),
                test_server.address.clone(),
                1,
                Some(0),
                Some(2), // Requesting three blocks, but only two will be returned
                std::io::sink(),
            )
            .await;
            let err = result.expect_err("Fetch should fail");
            assert_eq!(
                err.to_string(),
                "received fewer blocks than expected: expected 3, got 2",
            );
        }
        purge(tmpdir.path(), 1, None, None, Cursor::new("y")).unwrap(); // Clear the database for the next test
        // More blocks than expected
        {
            let mut mock_server = MockRpcServer::new();
            mock_server.expect_list().returning(|_| {
                Ok(tonic::Response::new(ChainRanges {
                    chain_ranges: vec![ChainRange {
                        chain_id: 1,
                        block_ranges: vec![proto_rpc::BlockRange { from: 0, to: 10 }],
                    }],
                }))
            });

            // Simulate a response that returns more blocks than requested
            mock_server.expect_get_block_range().returning(|_| {
                let response = (0..=4)
                    .map(|i| {
                        Ok(EncodedBlock {
                            number: i,
                            data: proto::Block::from(bertha_types::Block {
                                number: i,
                                ..bertha_types::Block::default_sonic()
                            })
                            .encode_to_vec(),
                        })
                    })
                    .collect::<Vec<_>>();
                Ok(tonic::Response::new(futures::stream::iter(response)))
            });

            let test_server = TestServer::new(mock_server).await;
            let result = fetch(
                tmpdir.path(),
                test_server.address.clone(),
                1,
                Some(0),
                Some(2), // Requesting three blocks, but five will be returned
                std::io::sink(),
            )
            .await;
            let err = result.expect_err("Fetch should fail");
            assert_eq!(
                err.to_string(),
                "received fewer blocks than expected: expected 3, got 5"
            );
        }
        purge(tmpdir.path(), 1, None, None, Cursor::new("y")).unwrap(); // Clear the database for the next test
        // Block number mismatch
        {
            let mut mock_server = MockRpcServer::new();
            mock_server.expect_list().returning(|_| {
                Ok(tonic::Response::new(ChainRanges {
                    chain_ranges: vec![ChainRange {
                        chain_id: 1,
                        block_ranges: vec![proto_rpc::BlockRange { from: 0, to: 10 }],
                    }],
                }))
            });

            // Simulate a response that returns blocks with unexpected numbers
            mock_server.expect_get_block_range().returning(|_| {
                let response = vec![
                    Ok(EncodedBlock {
                        number: 0,
                        data: proto::Block::from(bertha_types::Block {
                            number: 0,
                            ..bertha_types::Block::default_sonic()
                        })
                        .encode_to_vec(),
                    }),
                    Ok(EncodedBlock {
                        number: 2, // Should be 1
                        data: proto::Block::from(bertha_types::Block {
                            number: 2,
                            ..bertha_types::Block::default_sonic()
                        })
                        .encode_to_vec(),
                    }),
                ];
                Ok(tonic::Response::new(futures::stream::iter(response)))
            });

            let test_server = TestServer::new(mock_server).await;
            let result = fetch(
                tmpdir.path(),
                test_server.address.clone(),
                1,
                Some(0),
                Some(2), // Requesting three blocks, but second block will have wrong number
                std::io::sink(),
            )
            .await;
            let err = result.expect_err("Fetch should fail");
            assert!(
                err.to_string()
                    .contains("expected to receive block number 1, got 2")
            );
        }
    }

    #[tokio::test]
    async fn retrieves_block_range_correctly() {
        #[derive(Debug, Clone)]
        struct TestCase {
            from: Option<u64>, // From block number to fetch
            to: Option<u64>,   // To block number to fetch
            expected_ranges_to_fetch: Vec<proto_rpc::BlockRange>, /* Expected ranges to fetch
                                * from the server */
            expected_output: &'static str, // Expected output to be written
        }

        let tmpdir = tempfile::tempdir().unwrap();
        init_app_dir(tmpdir.path()).unwrap();

        let max_block_number: u64 = 40;
        let local_blocks_ranges = to_proto_block_range([
            1..=2,
            6..=9,
            15..=19,
            25..=29,
            32..=32,
            37..=37,
            39..=max_block_number,
        ]);
        let mut local_blocks = vec![];
        for local_block_ranges in &local_blocks_ranges {
            for i in local_block_ranges.from..=local_block_ranges.to {
                let b = bertha_types::Block {
                    number: i,
                    ..bertha_types::Block::default_sonic()
                };
                local_blocks.push(b.clone());
            }
        }
        let init_db = || {
            // Initialize the database
            let (_, db) = open_app_dir(tmpdir.path(), false).unwrap();
            db.delete_range(1, 0, u64::MAX).unwrap(); // Clear the database
            assert!(db.get_ranges_of_chain_id(1).unwrap().is_empty()); // make sure the database is empty
            // Insert the local blocks into the database
            for block in local_blocks.clone() {
                db.put(1, block).unwrap();
            }
        };

        let remote_blocks_ranges_cases = vec![
            to_proto_block_range([0..=40]), // All blocks in one range
            to_proto_block_range([0..=0, 3..=5, 10..=14, 20..=24, 30..=31, 33..=36, 38..=38]), /* Server has all blocks that are missing in client */
        ];

        let fetch_cases = vec![
            TestCase {
                from: None,
                to: None,
                expected_ranges_to_fetch: to_proto_block_range([
                    0..=0,
                    3..=5,
                    10..=14,
                    20..=24,
                    30..=31,
                    33..=36,
                    38..=38,
                ]),
                expected_output: "Fetched and wrote 21 blocks, total uncompressed size: 0 MiB\n",
            },
            TestCase {
                from: None,
                to: Some(23),
                expected_ranges_to_fetch: to_proto_block_range([0..=0, 3..=5, 10..=14, 20..=23]),
                expected_output: "Fetched and wrote 13 blocks, total uncompressed size: 0 MiB\n",
            },
            TestCase {
                from: Some(1),
                to: None,
                expected_ranges_to_fetch: to_proto_block_range([
                    3..=5,
                    10..=14,
                    20..=24,
                    30..=31,
                    33..=36,
                    38..=38,
                ]),
                expected_output: "Fetched and wrote 20 blocks, total uncompressed size: 0 MiB\n",
            },
            TestCase {
                from: Some(3),
                to: Some(23),
                expected_ranges_to_fetch: to_proto_block_range([3..=5, 10..=14, 20..=23]),
                expected_output: "Fetched and wrote 12 blocks, total uncompressed size: 0 MiB\n",
            },
            TestCase {
                from: Some(20),
                to: Some(24),
                expected_ranges_to_fetch: to_proto_block_range([20..=24]),
                expected_output: "Fetched and wrote 5 blocks, total uncompressed size: 0 MiB\n",
            },
            TestCase {
                from: Some(15),
                to: Some(19),
                expected_ranges_to_fetch: vec![],
                expected_output: "No blocks to fetch for chain ID 1 in range 15 to 19: All blocks are already available locally\n",
            },
        ];

        for remote_block_range in remote_blocks_ranges_cases {
            // Set what the server will return
            let list_response = ChainRanges {
                chain_ranges: vec![ChainRange {
                    chain_id: 1,
                    block_ranges: remote_block_range,
                }],
            };
            for TestCase {
                from,
                to,
                expected_ranges_to_fetch,
                expected_output,
            } in fetch_cases.clone()
            {
                init_db();
                let mut mock_server = MockRpcServer::new();
                mock_server.expect_list().returning({
                    let list_response = list_response.clone();
                    move |_| Ok(tonic::Response::new(list_response.clone()))
                });
                let mut sequence = mockall::Sequence::new();
                for proto_rpc::BlockRange { from, to } in &expected_ranges_to_fetch {
                    let mut range_response: Vec<Result<EncodedBlock, tonic::Status>> = vec![];
                    for i in *from..=*to {
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
                            let from = *from;
                            let to = *to;
                            move |request: &tonic::Request<BlockRangeRequest>| {
                                let req = request.get_ref();
                                req.chain_id == 1 && req.from == from && req.to == to
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
                fetch(tmpdir.path(), server.address.clone(), 1, from, to, &mut buf)
                    .await
                    .expect("Fetch should succeed");
                // Check that the output is as expected
                assert_eq!(String::from_utf8(buf).unwrap(), expected_output);
                // Check that the data were written to the database
                for proto_rpc::BlockRange { from, to } in expected_ranges_to_fetch {
                    for i in from..=to {
                        let (_, db) = open_app_dir(tmpdir.path(), true).unwrap();
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

    /// Converts an iterator of [BlockRange] into a [Vec<proto_rpc::BlockRange>].
    fn to_proto_block_range(
        ranges: impl IntoIterator<Item = BlockRange>,
    ) -> Vec<proto_rpc::BlockRange> {
        ranges.into_iter().map(Into::into).collect()
    }
}
