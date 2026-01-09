package app

import (
	"io"
	"os"
	"path/filepath"
)

// IsEmptyOrMissingDir returns true if a directory is missing or empty.
func IsEmptyOrMissingDir(path string) (bool, error) {
	_, err := os.Stat(path)
	if os.IsNotExist(err) {
		return true, nil // non-existent
	}
	if err != nil {
		return false, err
	}
	f, err := os.Open(path)
	if err != nil {
		return false, err
	}
	defer f.Close()

	_, err = f.Readdir(1) // try to read a single entry
	if err == io.EOF {
		return true, nil // empty
	}
	return false, err // either not empty or some other error
}

// DirSize computes the total size of all files in a directory.
func DirSize(path string) (int64, error) {
	var size int64
	err := filepath.Walk(path, func(_ string, info os.FileInfo, err error) error {
		if err != nil {
			return err
		}
		if !info.IsDir() {
			size += info.Size()
		}
		return nil
	})
	return size, err
}
