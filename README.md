# pgdumpx

**A bounded, byte-oriented row scanner for PostgreSQL custom-format dumps.**

> Status: early implementation. The workspace and baseline CI exist, but no crate or CLI release is available yet.

pgdumpx is a read-only Rust library and CLI for inspecting PostgreSQL custom-format (`pg_dump -Fc`) archives without restoring them into a database.

Its primary value is not merely opening an archive entry. pgdumpx is designed to select one table-data entry through the archive TOC, seek to it, stream decompression, parse PostgreSQL `COPY` text into logical rows and fields, evaluate an application-defined predicate, and stop as soon as the first matching row is found.

A representative target is:

> Find one order, user, or other record in a multi-gigabyte `-Fc` backup without starting PostgreSQL and without buffering the complete table.

[日本語 README](README.ja.md)

## Why pgdumpx?

A PostgreSQL custom archive contains a table of contents (TOC) and per-entry data positions. That makes selective **entry** access possible, but it does not provide a row-level value index.

pgdumpx composes the archive and row layers into one bounded inspection path:

```text
PostgreSQL custom archive
        │
        ▼
header + TOC metadata
        │
        ▼
select table-data entry + validated seek
        │
        ▼
streaming decompression
        │
        ▼
PostgreSQL COPY text parser
        │
        ├── borrowed rows and byte-oriented fields
        ├── COPY column metadata and name lookup
        ├── structural and scan-work limits
        └── predicate evaluation / first-match retrieval
```

The initial product direction emphasizes:

- **read-only parsing** of PostgreSQL Custom Format archives;
- no running PostgreSQL server, `libpq`, or `pg_restore` requirement at runtime;
- **lazy entry access** using `Read + Seek`;
- **streaming decompression** without buffering an entire table;
- **row-aware parsing** of normal pg_dump `COPY` text data;
- **borrowed rows and byte-oriented fields** without requiring UTF-8 or per-row ownership;
- **column-aware first-match filtering** without a SQL parser;
- typed, location-aware errors;
- structural, row-scan, and raw-extraction resource limits;
- a small core suitable for CLI and later language/data integrations;
- fixture-backed compatibility and benchmark-backed performance claims.

The project does not use “Pure Rust” as a blanket dependency guarantee. The default build is intended to remain independent of PostgreSQL runtime components; compression backend and native-build implications are documented separately. See [ADR 0007](docs/adr/0007-standalone-row-scanner-and-vertical-slices.md).

## Use cases

pgdumpx targets situations where a PostgreSQL dump is useful as an offline data source rather than only as restore input:

- inspect a production backup without starting PostgreSQL;
- find one record inside a large custom-format dump;
- extract one selected table-data stream from a multi-gigabyte archive;
- build backup verification and support/forensics tools;
- process customer-supplied dumps under explicit parser and work budgets;
- convert selected row streams into CSV, JSON Lines, Arrow, or Parquet in downstream tools;
- build Rust, CLI, Python, or analytical tooling on one reusable row-scanning core.

## Initial scope

v0.1 targets PostgreSQL Custom Format only:

```bash
pg_dump -Fc mydb > backup.dump
```

The final v0.1 compatibility target is archive format versions **1.14 through 1.16** with:

- none;
- gzip;
- LZ4;
- Zstandard.

Implementation starts with a narrow end-to-end slice for archive 1.16 and none/gzip, then expands to the complete matrix. See [ROADMAP.md](ROADMAP.md).

Row-aware access targets normal pg_dump table data represented as PostgreSQL `COPY` text. The following are explicitly outside the v0.1 row parser:

- `--inserts`;
- `--column-inserts`;
- INSERT output produced by `--rows-per-insert`;
- Binary COPY.

Unsupported logical representations must fail through a typed row-API error rather than being guessed as COPY text. Raw entry extraction may still be available when the archive entry itself is structurally readable.

The exact target-versus-verified matrix lives in [docs/COMPATIBILITY.md](docs/COMPATIBILITY.md). Until implementation fixtures pass through production code paths, compatibility entries are targets rather than release claims.

## Intended Rust API

The current Alpha 2 slice implements the metadata, row-streaming, first-match, public structural-limit, scan-limit, and error-taxonomy APIs shown below. Later v0.1 APIs remain subject to the roadmap.

```rust
use pgdumpx::{Archive, FieldRef};

let file = std::fs::File::open("backup.dump")?;
let mut archive = Archive::open(file)?;

println!("archive version: {:?}", archive.header().version());

for entry in archive.entries() {
    println!("{entry:?}");
}

let mut rows = archive.table_rows(b"public", b"orders")?;
while let Some(row) = rows.next_row()? {
    println!("{:?}", row);
}
```

`next_row(&mut self)` is intentionally not a normal `Iterator` method: each borrowed `Row` references a reusable internal buffer and remains valid only until the next mutable reader operation.

A primary v0.1 use case is finding the first matching row with explicit total-work budgets:

```rust
use pgdumpx::ScanLimits;

let mut rows = archive.table_rows(b"public", b"orders")?;
let order_number = rows
    .column_index(b"order_number")?
    .ok_or(/* application error */)?;

let scan_limits = ScanLimits::unlimited()
    .with_max_rows(100_000)
    .with_max_decompressed_bytes(64 * 1024 * 1024);

let row = rows.find_first_with_limits(scan_limits, |row| {
    row.field(order_number) == Some(FieldRef::Bytes(b"123456"))
})?;
```

`find_first` remains the convenience path without additional total-work budgets. `find_first_with_limits` uses the same streaming parser and predicate loop while enforcing the supplied `ScanLimits`.

Column lookup distinguishes three states:

```text
Ok(Some(index))  valid metadata, column found
Ok(None)         valid metadata, requested column absent
Err(...)         column layout unavailable or malformed
```

Both first-match methods perform a **streaming sequential scan**, not a database index lookup. The TOC enables direct access to the selected table-data entry, but rows must be decompressed and parsed in order from the beginning of that entry. A match can stop early; an absent or late match may process the complete selected table unless a configured budget ends the operation.

The API includes separate concepts for:

- structural/per-item limits;
- total row-scan work limits;
- decompressed-byte limits for raw entry extraction.

The implemented structural configuration is `Limits`. `Limits::default()` is finite and compatibility-oriented, `Archive::open` uses those defaults, and `Archive::open_with_limits` accepts stricter caller-selected TOC/string/dependency/row/field bounds through the same parser path.

The implemented total-work configuration is `ScanLimits`. `ScanLimits::default()` and `ScanLimits::unlimited()` leave both optional budgets unset. `max_rows = N` permits at most `N` complete rows to be yielded or evaluated, including a matching row. The decompressed-byte budget counts physical COPY bytes consumed by the parser—including field separators, row terminators, escape spellings, and the COPY terminator when consumed—not logical decoded field length or unread decoder/`BufRead` lookahead. A crossing row is not yielded or passed to the predicate, and exhaustion is returned as a typed resource error with limit and consumed-work context.

See [docs/API-DESIGN.md](docs/API-DESIGN.md).

## CLI

`inspect`, `list`, and `find` are implemented and consume the same public Rust library API rather than maintaining separate archive or COPY parsers. `extract` remains planned for a later Alpha 2 issue.

```bash
pgdumpx inspect backup.dump
pgdumpx list backup.dump
pgdumpx extract backup.dump public.orders
pgdumpx find backup.dump public.orders order_number 123456
pgdumpx find --max-rows 100000 --max-decompressed-bytes 67108864 \
  backup.dump public.orders order_number 123456
```

### `inspect` / `list`

`inspect <FILE>` prints archive version, compression, and entry/table/table-data counts as deterministic `key=value` lines. `list <FILE>` prints TOC entries in archive order as tab-separated dump ID, object type, schema, and name columns. Both commands stop at the library metadata-open path: they do not seek into TABLE DATA payloads, decompress entry data, or invoke the COPY row parser. Diagnostics are written to stderr and malformed archives exit non-zero.

### `extract` (planned)

`extract` writes the selected entry's **decompressed table-data body** to stdout as binary-safe bytes. It does not add schema DDL, a `COPY` statement wrapper, or a complete restorable SQL script.

The command uses the library's bounded raw-extraction path. Limit exhaustion is an error; output is not silently truncated.

### `find`

`find` is a narrow first-match equality command. It is not a SQL parser and does not introduce a general `WHERE` language.

The v0.1 form is:

```text
pgdumpx find [--max-rows <N>] [--max-decompressed-bytes <N>] <FILE> <SCHEMA.TABLE> <COLUMN> <VALUE>
```

The optional scan-limit flags precede `<FILE>`. Each accepts a positive decimal `u64`, may be specified at most once, and is independent of the other. Omitting a flag leaves that budget unlimited. `--max-rows <N>` counts complete rows evaluated by the library search path, including the matching row. `--max-decompressed-bytes <N>` uses the same parser-consumed physical COPY-byte accounting as the Rust API; it includes separators, row terminators, escape spellings, and a consumed COPY terminator, but excludes unread decompressor/buffer lookahead and decoded logical-length changes.

`<SCHEMA.TABLE>` contains exactly one ASCII `.` separator and non-empty schema and table components. SQL identifier quoting and escaping are not supported. Schema, table, column, and value arguments are UTF-8; the Rust API remains byte-oriented.

A match writes exactly one **normalized COPY text record** to stdout. Fields remain in COPY column order, are separated by ASCII tabs, and the record ends with LF. NULL is `\N`; an empty byte field is an empty field. Backslash, tab, LF, and CR are emitted as `\\`, `\t`, `\n`, and `\r`; other non-printable or non-ASCII bytes use three-digit octal escapes such as `\377`. This keeps stdout deterministic and ASCII-safe without lossy UTF-8 conversion. No match produces no output, and diagnostics are written only to stderr.

A resource limit is an operation failure, not a clean no-match result. It therefore writes a diagnostic to stderr and exits with `2+` even when no matching row was reached before exhaustion.

Stable exit behavior:

```text
0  match found
1  completed scan with no matching row
2+ usage, I/O, format, integrity, decompression, COPY, encoding, or resource error
```

## Architecture

Archive opening parses metadata and builds an entry-level index. Payloads remain lazy:

```text
Archive<R: Read + Seek>
        │
        ├── header + TOC parser
        ├── ArchiveIndex
        └── on-demand validated seek
                  │
                  ▼
          EntryDataReader
                  │
          streaming decompression
                  │
                  ▼
          COPY text row reader
                  │
                  ├── row iteration
                  └── first-match filtering
```

The implementation owns a narrow standalone read path so byte-oriented metadata, integrity checks, resource accounting, and row-parser errors follow one coherent model. Adjacent dump libraries remain useful references and differential-test comparators.

See [ARCHITECTURE.md](ARCHITECTURE.md).

## COPY text contract

`FieldRef::Bytes` exposes **logical field bytes after PostgreSQL COPY text escape decoding**. `\N` is represented as `FieldRef::Null`; an empty non-NULL field remains a zero-length byte string.

COPY record framing, escaping, column metadata, unsupported representations, and parser limits are specified in [docs/COPY-TEXT.md](docs/COPY-TEXT.md).

## Evidence policy

Compatibility and performance are claims that require evidence.

Valid archive fixtures must record:

- official `pg_dump` generator version;
- exact generation command;
- archive-format version and compression;
- checksum;
- fixture purpose and expected objects.

Benchmarks must record the fixture, command, hardware, compression, match position, and measurement method. README performance claims are added only after reproducible results exist.

## Related projects

- [`libpgdump`](https://github.com/gmr/libpgdump) — a Rust library for reading and writing PostgreSQL custom, directory, and tar dump formats.
- [`pgdumplib`](https://github.com/gmr/pgdumplib) — a Python library for reading and writing PostgreSQL custom-format dumps.

These projects cover adjacent PostgreSQL dump use cases. pgdumpx keeps a deliberately narrow contract around read-only, bounded, byte-oriented row inspection of Custom Format archives.

## Documentation map

Each document has one primary responsibility to reduce duplication and drift:

- [README](README.md) / [日本語 README](README.ja.md) — product value, status, examples, and high-level scope;
- [Requirements](docs/REQUIREMENTS.md) — normative v0.1 behavior and acceptance criteria;
- [Architecture](ARCHITECTURE.md) — internal boundaries and data flow;
- [Public API design](docs/API-DESIGN.md) — intended Rust API and exact API semantics;
- [Custom archive format notes](docs/PG-DUMP-CUSTOM-FORMAT.md) — upstream-derived archive behavior;
- [COPY text contract](docs/COPY-TEXT.md) — row and field byte semantics;
- [Compatibility matrix](docs/COMPATIBILITY.md) — target versus fixture-verified support;
- [Roadmap](ROADMAP.md) — delivery order;
- [Architecture Decision Records](docs/adr/) — accepted and superseded design decisions;
- [Contributing](CONTRIBUTING.md) — contribution and document-update policy;
- [Security policy](SECURITY.md) — vulnerability reporting and resource-threat model.

## Licensing

pgdumpx is licensed under either of:

- Apache License, Version 2.0; or
- MIT License;

at your option.

See [LICENSE-APACHE](LICENSE-APACHE) and [LICENSE-MIT](LICENSE-MIT).
