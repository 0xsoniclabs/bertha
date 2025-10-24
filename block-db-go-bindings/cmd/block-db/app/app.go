package app

import (
	"context"
	"log/slog"
	"os"
	"os/signal"
	"runtime"
	"syscall"

	"github.com/0xsoniclabs/tracy"
	"github.com/urfave/cli/v3"
)

func Run(args []string) error {
	return getApp().Run(context.Background(), args)
}

func getApp() *cli.Command {
	var profiler *profiler
	var tracyProfiler *tracyProfiler
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
			tracyProfiler = startTracyProfiler()
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
			tracyProfiler.Stop()
			return nil
		},
	}
}

type tracyProfiler struct {
	stop chan<- struct{}
	done <-chan struct{}
}

func startTracyProfiler() *tracyProfiler {
	stop := make(chan struct{})
	done := make(chan struct{})
	p := &tracyProfiler{
		stop: stop,
		done: done,
	}
	go func() {
		// Not sure whether this is necessary, but this ensures that the tracy
		// profiler is started and stopped on the same OS thread.
		defer close(done)
		runtime.LockOSThread()
		defer runtime.UnlockOSThread()
		tracy.StartupProfiler()
		<-stop
		tracy.ShutdownProfiler()
	}()
	return p
}

func (p *tracyProfiler) Stop() {
	close(p.stop)
	<-p.done
}
