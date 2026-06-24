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

package utils

import "github.com/schollz/progressbar/v3"

//go:generate mockgen -source=progress_indicator.go -destination=progress_indicator_mock.go -package=utils

// ProgressIndicator is an interface for mocking progressbar.ProgressBar interactions in tests.
type ProgressIndicator interface {
	Add(num int) error
}

// ProgressIndicatorFactory is an interface for mocking the creation of progressbar.ProgressBar in tests.
type ProgressIndicatorFactory interface {
	New(max int64, description ...string) ProgressIndicator
}

type ProgressBarFactory struct{}

func (f *ProgressBarFactory) New(max int64, description ...string) ProgressIndicator {
	return progressbar.Default(max, description...)
}
