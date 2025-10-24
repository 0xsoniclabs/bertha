package tracy

// #cgo CPPFLAGS: -Wno-unused-result -DTRACY_ENABLE=1 -DTRACY_DELAYED_INIT=1 -DTRACY_MANUAL_LIFETIME=1 -I${SRCDIR}/../../third-party/tracy/public
// #include "tracy.h"
import "C"

import (
	"runtime"
	"sync"
)

func StartupProfiler() {
	C.Bertha_TracyStartupProfiler()
}

func ShutdownProfiler() {
	C.Bertha_TracyShutdownProfiler()
}

type Zone int

func ZoneBegin(name string, color uint32) Zone {
	runtime.LockOSThread()
	tracyMutex.Lock()
	defer tracyMutex.Unlock()

	pc, filename, line, _ := runtime.Caller(1)
	funcname := runtime.FuncForPC(pc).Name()

	ret := C.Bertha_TracyZoneBegin(allocString(name), allocString(funcname),
		allocString(filename), C.uint(line), C.uint(color))

	return Zone(ret)
}

func (z Zone) End() {
	tracyMutex.Lock()
	C.Bertha_TracyZoneEnd(C.int(z))
	tracyMutex.Unlock()
	runtime.UnlockOSThread()
}

var tracyMutex sync.Mutex

var tracyStringsMap map[string]*C.char = make(map[string]*C.char)
var allocStringMutex sync.Mutex

func allocString(text string) *C.char {

	allocStringMutex.Lock()
	defer allocStringMutex.Unlock()

	val, ok := tracyStringsMap[text]
	if ok {
		return val
	}

	cgotext := C.CString(text)
	tracyStringsMap[text] = cgotext

	return cgotext
}

//go:linkname tracy_systemstack runtime.systemstack
func tracy_systemstack(fn func())
