# Changelog

This changelog records user-visible behavior, compatibility boundaries, and release-engineering changes for published versions of `pgdumpx`.

## [0.2.0]

This is the first public release of the `pgdumpx` library and `pgdumpx-cli` package. It includes the completed archive-reading foundation and the subsequent reusable extraction, filtering, scan-budget, security, and maintenance work.

### Added

- Read-only parsing of PostgreSQL custom-format archive metadata for archive versions 1.14 through 1.16.
- Bounded selected-entry streaming for none, gzip, LZ4, and Zstandard compression.
- Byte-oriented PostgreSQL COPY text row parsing with explicit NULL handling and exact named-column equality helpers.
- File-oriented archive helpers, owned table selectors, reusable extraction plans, sequential multi-table extraction, and metadata filtering.
- `pgdumpx` CLI commands for `inspect`, `list`, `extract`, and `find`, with finite default structural, raw-output, row, and decompressed-byte budgets.
- Reproducible official fixture provenance, compatibility differential checks, benchmark harnesses, fuzz targets, scheduled fuzz campaigns, and dependency advisory checks.

### Compatibility

- Rust 1.85.0 is the minimum supported Rust version.
- Linux, macOS, and Windows are exercised by the stable CI matrix.
- Row-aware APIs support PostgreSQL COPY text table data. Binary COPY and INSERT-based row decoding are intentionally unsupported; readable INSERT entries remain available through raw extraction.

### Security and reliability

- Project-authored Rust code forbids `unsafe`.
- Structural parsing, raw extraction, and row scanning expose separate typed resource limits.
- GitHub workflows use immutable action revisions, read-only permissions, disabled checkout credential persistence, concurrency cancellation, and finite job timeouts.
