package app

import (
	"context"

	"github.com/urfave/cli/v3"
)

func Run(args []string) error {
	return getApp().Run(context.Background(), args)
}

func getApp() *cli.Command {
	return &cli.Command{
		Name:  "block-db",
		Usage: "Block Database CLI",
		Commands: []*cli.Command{
			getVerifyCommand(),
		},
	}
}
