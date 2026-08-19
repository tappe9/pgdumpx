# pgdumpx

**Fast, safe, read-only PostgreSQL dump inspection and extraction in Pure Rust.**

> Status: design phase. No released crate or CLI exists yet.

pgdumpx is a reusable Rust engine for inspecting and extracting data from large PostgreSQL custom-format (`pg_dump -Fc`) archives without restoring them into a database.

The project is intentionally **read-only**. It is not a replacement for `pg_dump` or `pg_restore`. Its focus is efficient archive inspection, selective extraction, streaming table access, and safe parsing of untrusted dump files.

[日本語 README](README.ja.md)

## Why pgdumpx?

A PostgreSQL custom archive already contains a table of contents (TOC) and per-entry data offsets. For seekable inputs, pgdumpx aims to use that structure directly so callers can inspect metadata and open only the data entry they need instead of loading or restoring the entire dump.

The initial product direction emphasizes:

- **Pure Rust core** with no PostgreSQL server requirement;
- **read-only parsing** of PostgreSQL custom-format archives;
- **lazy data access** using `Read + Seek`;
- **streaming decompression** without buffering an entire table;
- **row-aware parsing** of PostgreSQL `COPY` text data;
- **typed, location-aware errors**;
- **resource limits** for attacker-controlled metadata and row sizes;
- a small core suitable for future CLI, Python, Arrow, and other bindings;
- benchmark-backed performance claims rather than unverified speed promises.

## Initial scope

v0.1 targets PostgreSQL custom-format archives:

```bash
pg_dump -Fc mydb > backup.dump
```

The initial compatibility target is archive format versions **1.14 through 1.16**. Support for older archive versions and other `pg_dump` formats is intentionally deferred.

Planned compression support:

- none;
- gzip;
- LZ4;
- Zstandard.

## Intended Rust API

The public API is still a design contract, not an implemented interface. The current direction is:

```rust
use pgdumpx::Archive;

let file = std::fs::File::open("backup.dump")?;
let mut archive = Archive::open(file)?;

println!("archive version: {}", archive.header().archive_version());

for entry in archive.entries() {
    println!("{} {:?} {}", entry.id(), entry.kind(), entry.name());
}

let mut rows = archive.table_rows("public", "orders")?;
while let Some(row) = rows.next_row()? {
    println!("{:?}", row);
}
```

The final API may change before the first release. See [docs/API-DESIGN.md](docs/API-DESIGN.md).

## Intended CLI

The CLI is a consumer of the same public Rust library API:

```bash
pgdumpx inspect backup.dump
pgdumpx list backup.dump
pgdumpx extract backup.dump public.orders
```

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
                 ▼
                Row
```

See [ARCHITECTURE.md](ARCHITECTURE.md) for the accepted initial architecture.

## Positioning

[`libpgdump`](https://github.com/gmr/libpgdump) is an existing Rust library that supports reading and writing custom, directory, and tar PostgreSQL dump formats, including lazy custom-archive access.

pgdumpx deliberately takes a narrower direction:

- read-only rather than read/write;
- custom format first rather than all formats;
- large-file inspection and extraction as the primary use case;
- row-aware `COPY` parsing as a core capability;
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
