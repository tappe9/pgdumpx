# ADR 0002: Support PostgreSQL custom format first

- Status: Accepted
- Date: 2026-08-19

## Context

PostgreSQL `pg_dump` can produce plain, custom, directory, and tar outputs. Supporting all formats from the beginning would expand implementation and testing scope before the project's main value proposition is proven.

Custom format (`-Fc`) provides structured metadata, TOC entries, compression, and stored data positions that make selective extraction especially valuable.

## Decision

v0.1 supports only PostgreSQL custom-format archives (`pg_dump -Fc`).

The initial compatibility target is archive versions 1.14, 1.15, and 1.16.

Directory, Tar, plain SQL, and older archive versions are deferred.

## Consequences

### Positive

- implementation can deeply test one binary format;
- stored offsets enable direct entry access on seekable sources;
- compression and row-aware extraction can be optimized around a clear use case;
- compatibility matrix remains manageable.

### Negative

- users with `-Fd`, `-Ft`, or plain dumps cannot use v0.1 directly;
- some abstractions may need extension when additional formats arrive.

## Guardrail

Do not invent a generic `ArchiveBackend` abstraction before a second format demonstrates which behaviors are actually common.
