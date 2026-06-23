// Copyright 2026 Sonic Operations Ltd
// This file is part of the Bertha testing infrastructure for Sonic.
//
// Bertha is free software: you can redistribute it and/or modify
// it under the terms of the GNU Lesser General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// Bertha is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Lesser General Public License for more details.
//
// You should have received a copy of the GNU Lesser General Public License
// along with Bertha. If not, see <http://www.gnu.org/licenses/>.

// Package utils provides general utility functions.
package utils

import (
	"errors"
	"io"
	"os"
	"path/filepath"
)

// IsEmptyOrMissingDir returns true if a directory is missing or empty.
func IsEmptyOrMissingDir(path string) (s bool, err error) {
	_, err = os.Stat(path)
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
	defer func() { err = errors.Join(err, f.Close()) }()

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
