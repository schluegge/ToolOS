# Spatial Window Checkpoint 18 Windows Verification

This directory contains an isolated, hash-verified integration slice derived from:

- final source commit: `d83879b30136616fc80f2033adaaf6757711770c`
- checkpoint candidate SHA-256: `bf286533a5d3316c8447a20ae64bc1f5a1e153a15ae772a6feffa46101977cb6`
- immutable base source commit: `34f8a9240950716580eeb497ea91ef2460eceece`
- immutable base archive SHA-256: `e7e5d7385f29539943e37cd910114ea5a693621df9cb34eeecfdaad3ddcf03c9`

The final source tree is reconstructed from the previously verified base archive plus one explicit, hash-verified `vcpkg.json` overlay. The Windows job validates the final 59-file tree before compilation and executes the bounded integration path:

`Named Pipe → generated Protobuf → CoreService → SQLite → canonical response`

A successful result verifies only this integration path and its direct dependencies. It does not establish that the complete Spatial Window product builds or runs on Windows.
