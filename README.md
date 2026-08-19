# pgdumpx

**Fast, safe, read-only PostgreSQL dump inspection and extraction in Pure Rust.**

> Status: design phase. No released crate or CLI exists yet.

pgdumpx is a reusable Rust engine for inspecting and extracting data from large PostgreSQL custom-format (`pg_dump -Fc`) archives without restoring them into a database.

The project is intentionally **read-only**. It is not a replacement for `pg_dump` or `pg_restore`. Its focus is efficient archive inspection, selective extraction, streaming table access, row-aware filtering, and safe parsing of untrusted dump files.

[日本語 README](README.ja.md)

## Why pgdumpx?

A PostgreSQL custom archive already contains a table of contents (TOC) and per-entry data offsets. For seekable inputs, pgdumpx aims to use that structure directly so callers can inspect metadata and open only the data entry they need instead of loading or restoring the entire dump.

The initial product direction emphasizes:

- **Pure Rust core** with no PostgreSQL server requirement;
- **read-only parsing** of PostgreSQL custom-format archives;
- **lazy data access** using `Read + Seek`;
- **streaming decompression** without buffering an entire table;
- **row-aware parsing** of PostgreSQL `COPY` text data;
- **column-aware first-match filtering** without restoring the dump;
- **typed, location-aware errors**;
- **resource limits** for attacker-controlled metadata and row sizes;
- a small core suitable for future CLI, Python, Arrow, and other bindings;
- benchmark-backed performance claims rather than unverified speed promises.

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
    .column_index(b"order_number")
    .ok_or(/* application error */)?;

let row = rows.find_first(|row| {
    row.field(order_number) == Some(FieldRef::Bytes(b"123456"))
})?;
```

`find_first` is a **streaming scan**, not a database index lookup. The custom archive lets pgdumpx seek directly to the selected table-data entry, but it does not contain a row-level index. The reader therefore decompresses and parses rows in order until the predicate matches, then stops immediately. Worst-case work is proportional to the selected table's data size.

The final API may change before the first release. See [docs/API-DESIGN.md](docs/API-DESIGN.md).

## Intended CLI

The CLI is a consumer of the same public Rust library API:

```bash
pgdumpx inspect backup.dump
pgdumpx list backup.dump
pgdumpx extract backup.dump public.orders
```

CLI work begins only after the parser core is usable and tested. A SQL-like query language is not part of v0.1.

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

## Positioning

[`libpgdump`](https://github.com/gmr/libpgdump) is an existing Rust library that supports reading and writing custom, directory, and tar PostgreSQL dump formats, including lazy custom-archive access.

pgdumpx deliberately takes a narrower direction:

- read-only rather than read/write;
- custom format only for the initial product direction;
- large-file inspection and selective extraction as primary use cases;
- row-aware `COPY` parsing and first-match filtering as core capabilities;
- explicit parser resource budgets and malformed-input hardening;
- performance work driven by repeatable benchmarks.

This project should not claim to outperform `libpgdump`, `pg_restore`, or another implementation until benchmark data demonstrates it.

## Documentation

- [Requirements](docs/REQUIREMENTS.md)
- [Architecture](ARCHITECTURE.md)
- [Public API design](docs/API-DESIGN.md)
- [PostgreSQL custom archive format notes](docs/PG-DUMP-CUSTOM-FORMAT.md)
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
