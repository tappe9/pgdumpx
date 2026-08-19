# ADR 0001: Build a Pure Rust read-only engine

- Status: Superseded by [ADR 0007](0007-standalone-row-scanner-and-vertical-slices.md)
- Date: 2026-08-19
- Superseded: 2026-08-19

## Context

The project exists to inspect and extract data from PostgreSQL dump archives without restoring them. PostgreSQL's own tools and existing Rust libraries cover broader read/write and restore-oriented use cases.

A narrow read-only engine can optimize API design, safety, streaming, and large-file behavior without taking responsibility for writing archives or reproducing `pg_restore`.

## Decision

pgdumpx will be a **Pure Rust, read-only** archive engine.

The core will not:

- write or mutate dump archives;
- execute SQL;
- connect to PostgreSQL;
- delegate archive parsing to PostgreSQL C code through FFI.

The initial implementation will avoid project-authored `unsafe`.

Pure Rust does not mean every transitive dependency is guaranteed to contain no internal `unsafe`; dependency review is a separate supply-chain concern.

## Consequences

### Positive

- smaller security and API surface;
- easier fuzzing and deterministic tests;
- portable library suitable for bindings;
- no PostgreSQL client/server runtime dependency;
- architecture can focus on selective extraction.

### Negative

- format compatibility logic must be maintained as PostgreSQL evolves;
- compression implementations may differ in performance from native libraries;
- archive writing and repair workflows are intentionally unavailable.

## Supersession note

The original decision used “Pure Rust” without defining whether it prohibited every native or FFI-backed transitive dependency. ADR 0007 retains the read-only, standalone Rust engine and no-PostgreSQL-runtime goals, but replaces that ambiguous label with explicit dependency and product-boundary requirements.
