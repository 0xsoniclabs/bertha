# Conventions

## General
- Keep changes as small as possible, and finish every change with a simplification pass.

## Tests
- `X_test.go` tests only `X.go`, in the order of its functions. Don't test behavior delegated to Sonic.
- Fold cases of the same shape into table tests: Go `map[string]struct{...}` + `t.Run`, Rust `rstest`.
- Error tests set up everything else to pass, so they fail only on the targeted check.
- Assertions without messages (unless required for understanding); Go: expected before actual, Rust: actual before expected.
- gomock: exact call counts (`.Times(n)` for n > 1, none for one call), no `.AnyTimes()`.
