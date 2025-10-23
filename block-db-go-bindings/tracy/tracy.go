package tracy

/*
#cgo CPPFLAGS: -I${SRCDIR}/../../third-party/tracy/public -DTRACY_ENABLE -DTRACY_DELAYED_INIT -DTRACY_MANUAL_LIFETIME -DTRACY_VERBOSE=1
#include "tracy.h"
#include <stdlib.h>
#include <stdio.h>
*/
import "C"

import (
	"runtime"
	"sync"
)

var tracyStringsMap map[string]*C.char = make(map[string]*C.char)
var allocStringMutex sync.Mutex
var tracyMutex sync.Mutex

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

func TracyStartupProfiler() {
	C.GoTracyStartupProfiler()
}

func TracyShutdownProfiler() {
	C.GoTracyShutdownProfiler()
}

func TracyZoneBegin(name string, color uint32) int {

	tracyMutex.Lock()

	pc, filename, line, _ := runtime.Caller(1)
	funcname := runtime.FuncForPC(pc).Name()

	ret := C.GoTracyZoneBegin(allocString(name), allocString(funcname),
		allocString(filename), C.uint(line), C.uint(color))

	tracyMutex.Unlock()
	return int(ret)
}

func TracyZoneEnd(c int) {
	tracyMutex.Lock()
	C.GoTracyZoneEnd(C.int(c))
	tracyMutex.Unlock()
}
