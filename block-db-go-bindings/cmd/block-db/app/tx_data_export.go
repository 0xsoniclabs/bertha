package app

import (
	"context"
	"fmt"
	"log/slog"
	"math/big"

	"github.com/0xsoniclabs/blockdb"
	"github.com/urfave/cli/v3"
)

func getTxDataExportCommand() *cli.Command {
	return &cli.Command{
		Name:   "tx-data-export",
		Usage:  "export transaction data from the block database",
		Action: runTxDataExport,
		Flags: []cli.Flag{
			blockDatabaseDirectoryFlag,
			chainIdFlag,
			endBlockFlag,
		},
	}
}

func runTxDataExport(ctx context.Context, cmd *cli.Command) error {
	dbDir := cmd.String(blockDatabaseDirectoryFlag.Name)
	// Open the block database.
	slog.Info("Opening block database", "directory", dbDir)
	database, err := blockdb.OpenRocksDBForReading(dbDir)
	if err != nil {
		slog.Error("Failed to open block database", "error", err)
		return err
	}
	defer func() {
		slog.Info("Closing block database", "directory", dbDir)
		if err := database.Close(); err != nil {
			slog.Error("Failed to close block database", "error", err)
		}
	}()

	chainID := cmd.Uint64(chainIdFlag.Name)
	endBlock := cmd.Uint64(endBlockFlag.Name)
	slog.Info("Exporting transaction data", "chainId", chainID, "endBlock", endBlock)

	fmt.Printf("block, timestamp, base_fee, gas_limit, gas_used, gas_price, status\n")

	for block, err := range database.GetRange(chainID, 0, endBlock) {
		if err != nil {
			slog.Error("Failed to read block from database", "block", block, "error", err)
			return err
		}

		if ctx.Err() != nil {
			slog.Warn("Context cancelled, stopping export", "block", block.Number)
			return ctx.Err()
		}

		if block.Number%100000 == 0 {
			slog.Info("Exporting block", "block", block.Number)
		}

		txs := block.Transactions
		receipts := block.Receipts
		if len(txs) != len(receipts) {
			slog.Warn("Block has different number of transactions and receipts", "block", block, "txCount", len(txs), "receiptCount", len(receipts))
			return fmt.Errorf("block %d has different number of transactions and receipts: %d transactions, %d receipts", block.Number, len(txs), len(receipts))
		}

		gethBlock, err := ConvertToGethBlock(block)
		if err != nil {
			slog.Error("Failed to convert block to geth format", "block", block, "error", err)
			return fmt.Errorf("failed to convert block %d to geth format: %w", block.Number, err)
		}

		baseFee := new(big.Int).SetBytes(block.BaseFeePerGas)

		cumulativeGasUsed := uint64(0)
		for i, tx := range gethBlock.Transactions() {
			receipt := block.Receipts[i]

			newCumulativeGasUsed := receipt.GetCumulativeGasUsed()
			gasUsed := newCumulativeGasUsed - cumulativeGasUsed
			cumulativeGasUsed = newCumulativeGasUsed

			fmt.Printf("%d, %d, %v, %d, %d, %v, %d\n",
				block.Number,
				block.Timestamp,
				baseFee,
				tx.Gas(),
				gasUsed,
				tx.GasPrice(),
				receipt.GetStatus(),
			)
		}
	}

	return nil
}
