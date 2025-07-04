package app

import "github.com/urfave/cli/v3"

var (
	blockDatabaseDirectoryFlag = &cli.StringFlag{
		Name:    "database-dir",
		Aliases: []string{"db"},
		Usage:   "Path to the block database directory",
		Value:   "./.blockdb",
	}

	chainIdFlag = &cli.Uint64Flag{
		Name:    "chain-id",
		Aliases: []string{"c"},
		Usage:   "Chain ID to verify",
		Value:   146, // Default to Sonic mainnet chain ID
	}
)
