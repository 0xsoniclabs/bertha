package block_db

import (
	"os"
	"testing"
)

func fillDbWithBlocks(chainId uint64, blockNumbers []uint64) (string, error) {
	path, err := os.MkdirTemp("", "blockdb-*")
	if err != nil {
		return "", err
	}

	db, err := createDb(path)
	if err != nil {
		return "", err
	}

	for _, num := range blockNumbers {
		block := Block{Number: num}
		if err := db.putBlock(chainId, &block); err != nil {
			return "", err
		}
	}

	db.close()

	return path, nil
}

func TestGetBlocksReturnsBlockIfItExists(t *testing.T) {
	chainId := uint64(3)
	blockNumbers := []uint64{1, 2, 3}

	path, err := fillDbWithBlocks(chainId, blockNumbers)
	if err != nil {
		t.Fatalf("failed to create db: %v", err)
	}

	// db now contains blocks 1, 2, and 3 for chainId 3

	db, err := OpenDb(path)
	if err != nil {
		t.Fatalf("failed to open db: %v", err)
	}

	// try to get block for non-existing chainId
	block, _ := db.GetBlock(0, 0)
	if block != nil {
		t.Fatalf("got block but db contains no blocks for this chain id")
	}

	// try to get block with non-existing block number
	block, _ = db.GetBlock(chainId, 4)
	if block != nil {
		t.Fatalf("got block but db contains no blocks with block number 4")
	}

	// get existing block
	block, err = db.GetBlock(chainId, 1)
	if err != nil {
		t.Fatalf("failed to retrieve block: %v", err)
	}
	if block == nil {
		t.Fatalf("expected block to exist, but got nil")
	}
	if block.Number != 1 {
		t.Fatalf("expected block number 1, got %d", block.Number)
	}

	db.Close()
}

func TestGetBlocksReturnsExistingSubRange(t *testing.T) {
	chainId := uint64(3)
	blockNumbers := []uint64{1, 2, 3}

	path, err := fillDbWithBlocks(chainId, blockNumbers)
	if err != nil {
		t.Fatalf("failed to create db: %v", err)
	}

	// db now contains blocks 1, 2, and 3 for chainId 3

	db, err := OpenDb(path)
	if err != nil {
		t.Fatalf("failed to open db: %v", err)
	}

	// get blocks for non-existing chainId
	count := 0
	for range db.GetBlocks(0, 0, 10) {
		count++
	}
	if count > 0 {
		t.Fatalf("got blocks for non existing chain id")
	}

	// get blocks for range outside of existing blocks
	count = 0
	for range db.GetBlocks(chainId, 1000, 1005) {
		count++
	}
	if count > 0 {
		t.Fatalf("got blocks for range outside of existing blocks")
	}

	// get blocks for for range which contains existing blocks
	count = 0
	for range db.GetBlocks(chainId, 0, 6) {
		count++
	}
	if count != 3 {
		t.Fatalf("got invalid number of blocks")
	}

	// get blocks for range which starts before existing blocks
	// and ends within existing blocks
	count = 0
	for range db.GetBlocks(chainId, 0, 2) {
		count++
	}
	if count != 2 {
		t.Fatalf("got invalid number of blocks")
	}

	// get blocks for range which starts within existing blocks
	// and ends after existing blocks
	count = 0
	for range db.GetBlocks(chainId, 2, 6) {
		count++
	}
	if count != 2 {
		t.Fatalf("got invalid number of blocks")
	}

	db.Close()
}
