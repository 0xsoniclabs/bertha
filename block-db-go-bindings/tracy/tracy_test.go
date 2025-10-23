package tracy

import (
	"fmt"
	"runtime"
	"testing"
	"time"
)

func TestStartStop(t *testing.T) {
	runtime.LockOSThread()
	defer runtime.UnlockOSThread()

	TracyStartupProfiler()
	defer TracyShutdownProfiler()

	zone := TracyZoneBegin("outer", 0xff0000)
	defer TracyZoneEnd(zone)
	fmt.Printf("Zone: %v\n", zone)

	time.Sleep(200 * time.Millisecond)

	inner()

	/*
		//zone2 := ZoneBegin("inner", 0xff0000)
		//defer zone2.End()
		//fmt.Printf("Zone: %v\n", zone2)

		time.Sleep(200 * time.Millisecond)
		//zone2.End()
	*/
}

func inner() {
	zone := TracyZoneBegin("innerFunc", 0x00ff00)
	defer TracyZoneEnd(zone)

	time.Sleep(100 * time.Millisecond)
}
