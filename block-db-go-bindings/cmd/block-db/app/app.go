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
	var profiler *profiler
	return &cli.Command{
		Name:  "block-db",
		Usage: "Block Database CLI",
		Commands: []*cli.Command{
			getReplayCommand(),
			getVerifyCommand(),
		},
		Flags: []cli.Flag{
			cpuProfileFlag,
		},
		Before: func(ctx context.Context, cmd *cli.Command) (context.Context, error) {
			var err error
			profiler, err = StartCpuProfile(cmd.String(cpuProfileFlag.Name))
			if err != nil {
				return ctx, err
			}
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
		After: func(_ context.Context, cmd *cli.Command) error {
			if profiler != nil {
				return profiler.Stop()
			}
			return nil
		},
	}
}
