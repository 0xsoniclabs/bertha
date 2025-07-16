package app

import "github.com/urfave/cli/v3"

var (
	blockDatabaseDirectoryFlag = &cli.StringFlag{
		Name:    "database-dir",
		Aliases: []string{"db"},
		Usage:   "Path to the block database directory",
		Value:   "./.blockdb",
	}
)
