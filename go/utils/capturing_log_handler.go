// Copyright 2026 Sonic Operations Ltd
// This file is part of the Sonic Client
//
// Sonic is free software: you can redistribute it and/or modify
// it under the terms of the GNU Lesser General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// Sonic is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Lesser General Public License for more details.
//
// You should have received a copy of the GNU Lesser General Public License
// along with Sonic. If not, see <http://www.gnu.org/licenses/>.

package utils

import (
	"context"
	"log/slog"
)

// CapturingLogHandler is a slog.Handler that captures all log records.
type CapturingLogHandler struct {
	records []slog.Record
}

func (h *CapturingLogHandler) Records() []slog.Record {
	return h.records
}

func (h *CapturingLogHandler) Enabled(_ context.Context, _ slog.Level) bool { return true }
func (h *CapturingLogHandler) Handle(_ context.Context, r slog.Record) error {
	h.records = append(h.records, r)
	return nil
}
func (h *CapturingLogHandler) WithAttrs(_ []slog.Attr) slog.Handler { return h }
func (h *CapturingLogHandler) WithGroup(_ string) slog.Handler      { return h }
