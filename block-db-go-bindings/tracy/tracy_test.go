package tracy

import (
	"sync"
	"testing"
	"time"
)

func TestStartStop(t *testing.T) {
	StartupProfiler()
	defer ShutdownProfiler()

	zone := ZoneBegin("outer", 0xff0000)
	defer zone.End()

	time.Sleep(200 * time.Millisecond)

	inner()

	const N = 5
	wg := sync.WaitGroup{}
	wg.Add(N)
	for range N {
		go func() {
			defer wg.Done()
			zone := ZoneBegin("goroutine", 0x0000ff)
			defer zone.End()
			inner()
		}()
	}
	wg.Wait()
}

func inner() {
	zone := ZoneBegin("innerFunc", 0x00ff00)
	defer zone.End()

	time.Sleep(100 * time.Millisecond)
}
