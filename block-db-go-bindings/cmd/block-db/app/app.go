package app

import (
	"context"
	"log/slog"
	"os"
	"os/signal"
	"syscall"

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
			getReplayCommand(),
			getVerifyCommand(),
		},
		Before: func(ctx context.Context, _ *cli.Command) (context.Context, error) {
			ctx, cancel := context.WithCancel(ctx)
			go func() {
				sigs := make(chan os.Signal, 1)
				signal.Notify(sigs, syscall.SIGINT, syscall.SIGTERM)
				select {
				case <-ctx.Done():
					return
				case <-sigs:
					slog.Warn("Received interrupt signal")
					cancel()
				}
			}()
			return ctx, nil
		},
	}
}
