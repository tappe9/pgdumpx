# pgdumpx

**A streaming, row-aware reader for PostgreSQL custom-format dumps, written in Pure Rust.**

> Status: design phase. No released crate or CLI exists yet.

pgdumpx is a reusable Rust engine for inspecting and extracting data from large PostgreSQL custom-format (`pg_dump -Fc`) archives without restoring them into a database.

The project is intentionally **read-only**. It is not a replacement for `pg_dump` or `pg_restore`. Its focus is bounded, selective inspection: parse archive metadata, open only the table-data entry you need, stream decompression, interpret PostgreSQL `COPY` text as rows and fields, and stop a scan as soon as an application-defined predicate matches.

For example, a caller should be able to inspect a multi-gigabyte archive, select `public.orders`, resolve the `order_number` column, and find the first matching row without restoring the database or buffering the complete table.

[日本語 README](README.ja.md)

## Why pgdumpx?

A PostgreSQL custom archive already contains a table of contents (TOC) and per-entry data offsets. pgdumpx uses that archive structure as the foundation for a row-aware inspection pipeline:

```text
PostgreSQL custom archive
        │
        ▼
header + TOC metadata
        │
        ▼
select table-data entry + seek
        │
        ▼
streaming decompression
        │
        ▼
PostgreSQL COPY text parser
        │
        ├── borrowed rows and byte-oriented fields
        ├── COPY column metadata + name lookup
        └── streaming predicates / first-match retrieval
```

The initial product direction emphasizes:

- **Pure Rust core** with no PostgreSQL server requirement;
- **read-only parsing** of PostgreSQL custom-format archives;
- **lazy entry access** using `Read + Seek`;
- **streaming decompression** without buffering an entire table;
- **row-aware parsing** of PostgreSQL `COPY` text data;
- **borrowed rows and byte-oriented fields** so parsing does not require UTF-8 or per-row ownership;
- **column-aware first-match filtering** without restoring the dump;
- **typed, location-aware errors**;
- **per-item and scan-work resource limits** for attacker-controlled or unexpectedly large input;
- a small core suitable for future CLI, Python, Arrow, and other bindings;
- benchmark-backed performance claims rather than unverified speed promises.

## Use cases

pgdumpx is intended for situations where a PostgreSQL dump is useful as an offline data source rather than only as restore input. Examples include:

- inspect a production backup without starting a PostgreSQL server;
- find one order, user, or other record inside a large custom-format dump;
- extract one selected table from a multi-gigabyte archive;
- build backup verification and support/forensics tools;
- inspect customer-provided dumps with explicit parser and scan budgets;
- convert selected table streams into other formats in downstream tools;
- build Rust, CLI, Python, or analytical tooling on one reusable archive reader.

A representative target is: **find one record in a very large `-Fc` backup without restoring PostgreSQL and without loading the selected table into memory.**

## Initial scope

v0.1 targets PostgreSQL custom-format archives only:

```bash
pg_dump -Fc mydb > backup.dump
```

The initial compatibility target is archive format versions **1.14 through 1.16**. Support for older archive versions and other `pg_dump` formats is intentionally deferred and is not required for the project to be useful.

Planned compression support:

- none;
- gzip;
- LZ4;
- Zstandard.

Row-aware v0.1 access targets normal pg_dump table data represented as PostgreSQL `COPY` text. INSERT-based dump modes such as `--inserts`, `--column-inserts`, and INSERT output produced with `--rows-per-insert` are not parsed as rows in v0.1 and must fail explicitly through the row API rather than being guessed as COPY input. Binary COPY decoding is also deferred.

The exact target-versus-verified matrix lives in [docs/COMPATIBILITY.md](docs/COMPATIBILITY.md). Until implementation fixtures exist, compatibility entries are targets rather than release claims.

## Intended Rust API

The public API is still a design contract, not an implemented interface. The current direction is:

```rust
use pgdumpx::{Archive, FieldRef};

let file = std::fs::File::open("backup.dump")?;
let mut archive = Archive::open(file)?;

println!("archive version: {}", archive.header().archive_version());

for entry in archive.entries() {
    println!("{} {:?} {}", entry.id(), entry.kind(), entry.name());
}

let mut rows = archive.table_rows(b"public", b"orders")?;
while let Some(row) = rows.next_row()? {
    println!("{:?}", row);
}
```

A primary v0.1 use case is finding the first matching row without restoring the archive:

```rust
let mut rows = archive.table_rows(b"public", b"orders")?;
let order_number = rows
    .column_index(b"order_number")?
    .ok_or(/* application error */)?;

let row = rows.find_first(|row| {
    row.field(order_number) == Some(FieldRef::Bytes(b"123456"))
})?;
```

Column lookup distinguishes three states: `Ok(Some(index))` when metadata is valid and the column exists, `Ok(None)` when metadata is valid but the requested name is absent, and `Err(...)` when the supported COPY column layout cannot be derived.

`find_first` is a **streaming scan**, not a database index lookup. The custom archive lets pgdumpx seek directly to the selected table-data entry, but it does not contain a row-level index. The reader therefore decompresses and parses rows in order until the predicate matches, then stops immediately. Worst-case work is proportional to the selected table's data size.

Long-running scans can be given operation-level work budgets such as maximum rows and maximum decompressed bytes, in addition to per-row allocation limits. This keeps bounded-memory parsing from becoming an implicitly unbounded CPU/decompression operation when input is untrusted.

The final API may change before the first release. See [docs/API-DESIGN.md](docs/API-DESIGN.md).

## Intended CLI

The CLI is a consumer of the same public Rust library API:

```bash
pgdumpx inspect backup.dump
pgdumpx list backup.dump
pgdumpx extract backup.dump public.orders
pgdumpx find backup.dump public.orders order_number 123456
```

`find` is intentionally a narrow first-match equality command that demonstrates the row-aware core. It is not a SQL parser and does not introduce a general `WHERE` language in v0.1.

CLI work begins only after the parser core is usable and tested.

## Architecture

The core opens a seekable archive, parses only the header and TOC into an in-memory index, and defers entry data reads until requested:

```text
PostgreSQL custom archive
        │
        ▼
  Archive<R: Read + Seek>
        │
        ├── Header parser
        ├── TOC parser ─────► ArchiveIndex
        │                         │
        │                         ├── metadata queries
        │                         └── entry offsets
        │
        └── on-demand seek
                 │
                 ▼
          EntryDataReader
                 │
          decompression
                 │
                 ▼
          COPY text parser
                 │
                 ├── row iteration
                 └── first-match filtering
```

See [ARCHITECTURE.md](ARCHITECTURE.md) for the accepted initial architecture.

## COPY text contract

The byte-oriented API exposes **logical field bytes after PostgreSQL COPY text escape decoding**. `\N` is represented as `FieldRef::Null`; empty non-NULL fields remain zero-length byte strings.

COPY record framing, escaping, column metadata, unsupported table-data representations, and parser limits are specified separately in [docs/COPY-TEXT.md](docs/COPY-TEXT.md).

## Related projects

- [`libpgdump`](https://github.com/gmr/libpgdump) — a Rust library for reading and writing PostgreSQL custom, directory, and tar dump formats.
- [`pgdumplib`](https://github.com/gmr/pgdumplib) — a Python library for reading and writing PostgreSQL custom-format dumps.

These projects cover adjacent PostgreSQL dump use cases. pgdumpx keeps its own deliberately narrow contract around read-only, bounded, row-aware inspection of custom-format archives.

## Documentation

- [Requirements](docs/REQUIREMENTS.md)
- [Architecture](ARCHITECTURE.md)
- [Public API design](docs/API-DESIGN.md)
- [PostgreSQL custom archive format notes](docs/PG-DUMP-CUSTOM-FORMAT.md)
- [COPY text parser contract](docs/COPY-TEXT.md)
- [Compatibility matrix](docs/COMPATIBILITY.md)
- [Roadmap](ROADMAP.md)
- [Architecture Decision Records](docs/adr/)
- [Contributing](CONTRIBUTING.md)
- [Security policy](SECURITY.md)

## Licensing

pgdumpx is licensed under either of:

- Apache License, Version 2.0; or
- MIT License;

at your option.

See [LICENSE-APACHE](LICENSE-APACHE) and [LICENSE-MIT](LICENSE-MIT).
