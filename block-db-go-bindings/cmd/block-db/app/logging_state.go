package app

import (
	"context"
	"fmt"
	"io"

	cc "github.com/0xsoniclabs/carmen/go/common"
	"github.com/0xsoniclabs/carmen/go/common/amount"
	"github.com/0xsoniclabs/carmen/go/common/future"
	"github.com/0xsoniclabs/carmen/go/common/result"
	"github.com/0xsoniclabs/carmen/go/common/witness"
	"github.com/0xsoniclabs/carmen/go/state"
	carmen "github.com/0xsoniclabs/carmen/go/state"
)

var _ carmen.State = (*LoggingState)(nil)

type LoggingState struct {
	inner carmen.State
}

func NewLoggingState(inner carmen.State) *LoggingState {
	return &LoggingState{
		inner: inner,
	}
}

func (s *LoggingState) Exists(address cc.Address) (bool, error) {
	got, err := s.inner.Exists(address)
	fmt.Printf("Exists address=%s exists=%v err=%v\n", address, got, err)
	return got, err
}

func (s *LoggingState) GetBalance(address cc.Address) (amount.Amount, error) {
	got, err := s.inner.GetBalance(address)
	fmt.Printf("GetBalance address=%s balance=%v err=%v\n", address, got, err)
	return got, err
}

func (s *LoggingState) GetNonce(address cc.Address) (cc.Nonce, error) {
	got, err := s.inner.GetNonce(address)
	fmt.Printf("GetNonce address=%s nonce=%v err=%v\n", address, got, err)
	return got, err
}

func (s *LoggingState) GetStorage(address cc.Address, key cc.Key) (cc.Value, error) {
	got, err := s.inner.GetStorage(address, key)
	fmt.Printf("GetStorage address=%s key=%v value=%v err=%v\n", address, key, got, err)
	return got, err
}

func (s *LoggingState) GetCode(address cc.Address) ([]byte, error) {
	got, err := s.inner.GetCode(address)
	fmt.Printf("GetCode address=%s codeSize=%d err=%v\n", address, len(got), err)
	return got, err
}

func (s *LoggingState) GetCodeSize(address cc.Address) (int, error) {
	got, err := s.inner.GetCodeSize(address)
	fmt.Printf("GetCodeSize address=%s codeSize=%d err=%v\n", address, got, err)
	return got, err
}

func (s *LoggingState) GetCodeHash(address cc.Address) (cc.Hash, error) {
	got, err := s.inner.GetCodeHash(address)
	fmt.Printf("GetCodeHash address=%s codeHash=%v err=%v\n", address, got, err)
	return got, err
}

func (s *LoggingState) HasEmptyStorage(addr cc.Address) (bool, error) {
	got, err := s.inner.HasEmptyStorage(addr)
	fmt.Printf("HasEmptyStorage address=%s hasEmptyStorage=%v err=%v\n", addr, got, err)
	return got, err
}

func (s *LoggingState) Apply(block uint64, update cc.Update) error {
	fmt.Printf("Apply block=%d update=%v\n", block, update)
	return s.inner.Apply(block, update)
}

func (s *LoggingState) GetHash() (cc.Hash, error) {
	got, err := s.inner.GetHash()
	fmt.Printf("GetHash hash=%v err=%v\n", got, err)
	return got, err
}

func (s *LoggingState) GetCommitment() future.Future[result.Result[cc.Hash]] {
	return s.inner.GetCommitment()
}

func (s *LoggingState) Flush() error {
	return s.inner.Flush()
}

func (s *LoggingState) Close() error {
	return s.inner.Close()
}

func (s *LoggingState) GetMemoryFootprint() *cc.MemoryFootprint {
	return s.inner.GetMemoryFootprint()
}

func (s *LoggingState) GetArchiveState(block uint64) (state.State, error) {
	return s.inner.GetArchiveState(block)
}

func (s *LoggingState) GetArchiveBlockHeight() (height uint64, empty bool, err error) {
	return s.inner.GetArchiveBlockHeight()
}

func (s *LoggingState) Check() error {
	return s.inner.Check()
}

func (s *LoggingState) CreateWitnessProof(address cc.Address, keys ...cc.Key) (witness.Proof, error) {
	return s.inner.CreateWitnessProof(address, keys...)
}

func (s *LoggingState) Export(ctx context.Context, out io.Writer) (cc.Hash, error) {
	return s.inner.Export(ctx, out)
}
