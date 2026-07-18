# Spatial Window Checkpoint 18 Windows Verification

This directory contains an isolated, hash-verified integration slice derived from:

- source commit: `34f8a9240950716580eeb497ea91ef2460eceece`
- checkpoint candidate SHA-256: `76f9a490f7bec6badc8413a6fc620f126b914ecac6eb9f36ba337627d9ec1664`
- payload archive SHA-256: `e7e5d7385f29539943e37cd910114ea5a693621df9cb34eeecfdaad3ddcf03c9`

The Windows job restores and verifies all 59 source files before compilation. It then executes the bounded integration path:

`Named Pipe → generated Protobuf → CoreService → SQLite → canonical response`

A successful result verifies only this integration path and its direct dependencies. It does not establish that the complete Spatial Window product builds or runs on Windows.
