# ADR 0007: Standalone row scanner and vertical-slice delivery

- Status: Accepted
- Date: 2026-08-19
- Supersedes: ADR 0001 terminology and dependency wording
- Refines: ADR 0003 and ADR 0006 implementation order

## Context

A pre-implementation project review found two risks in the accepted documentation.

First, “Pure Rust” was used as a headline without a precise product requirement behind it. It could be interpreted either as “does not require PostgreSQL, libpq, or `pg_restore`” or as a guarantee that every transitive dependency and compression backend contains no native code or FFI. The first interpretation is important to pgdumpx; the second was not an intentional compatibility promise.

Second, lazy TOC loading, per-entry seeking, and streaming decompression are useful but are no longer sufficient product differentiation on their own. Existing PostgreSQL dump libraries can provide adjacent archive-level capabilities. pgdumpx's distinctive value is the complete bounded row-inspection path:

```text
select table entry
    -> seek
    -> stream decompression
    -> parse COPY text
    -> resolve columns
    -> evaluate row predicates
    -> stop at the first match
```

The previous roadmap implemented archive layers horizontally and postponed the first visible row-search user story until late in v0.1. That creates a risk of producing a partial archive reader that is difficult to evaluate and does not yet demonstrate the project's specialization.

## Decision

### Product position

pgdumpx is a **bounded, byte-oriented row scanner for PostgreSQL Custom Format archives**.

The project remains:

- read-only;
- focused on `pg_dump -Fc`;
- optimized for selective, seekable access;
- independent of PostgreSQL server processes and database connections;
- explicit that row search is sequential within the selected table-data entry.

Archive metadata parsing, TOC lookup, seeking, and streaming decompression are foundations of the row-aware workflow rather than standalone product claims.

### Runtime and dependency boundary

The default pgdumpx build must not require:

- a running PostgreSQL server;
- `libpq`;
- `pg_restore` or other PostgreSQL executables at runtime;
- project-authored C code;
- project-authored `unsafe` without a separately accepted ADR.

The project does not use “Pure Rust” as a blanket guarantee about every transitive dependency. Compression backends are implementation details and must be selected through correctness, maintenance, portability, build, and benchmark evidence. Any backend that introduces a material native build or distribution constraint must be documented and, where practical, feature-gated.

### Standalone parser

pgdumpx will implement its own narrow read path rather than use another dump library as a mandatory archive backend.

Reasons:

- archive metadata and row values must remain byte-oriented where encoding is not guaranteed;
- structural, decompression, and scan limits must be enforceable inside the production path;
- location-aware errors and integrity checks must follow one coherent model;
- unsupported logical table-data representations must be rejected before COPY parsing;
- public API evolution should not be constrained by a broader read/write archive model.

Adjacent libraries may be used as research references and differential-test comparators. They are not framed as inferior or incompatible projects.

### Vertical-slice implementation order

The first implementation target is an end-to-end slice that demonstrates the user value before broad compatibility work:

1. Cargo workspace and CI;
2. archive version 1.16 header and minimum TOC fields;
3. one `(schema, table)` lookup and validated entry seek;
4. none and gzip entry streaming;
5. COPY text rows and supported column metadata;
6. `find_first` and the `pgdumpx find` CLI path.

After that slice works through official fixtures, implementation broadens to full limits/error semantics, archive versions 1.14 and 1.15, LZ4/Zstandard, fuzzing, and benchmarks.

The final v0.1 scope is not reduced by this ordering. It changes when usable evidence appears.

## Consequences

### Positive

- the project's differentiator is visible in its first usable alpha;
- “no PostgreSQL runtime dependency” remains explicit without making an accidental supply-chain guarantee;
- archive and COPY safety requirements remain under pgdumpx control;
- CLI behavior becomes an early end-to-end acceptance test;
- compatibility breadth is added after the core row-scanning path can be measured and validated;
- related projects can be acknowledged without turning the README into a competitive comparison document.

### Trade-offs

- pgdumpx must maintain its own format compatibility implementation;
- the first vertical slice supports fewer archive/compression combinations than the final v0.1 target;
- early APIs may evolve as real fixtures expose better ownership or error boundaries;
- dependency policy must be documented per backend rather than summarized by one label.

## Documentation impact

- README files lead with row scanning rather than “Pure Rust” or archive streaming alone.
- `ROADMAP.md` uses vertical alpha/beta slices.
- `API-DESIGN.md` defines opaque public metadata types, raw extraction limits, and exact scan accounting semantics.
- `REQUIREMENTS.md` defines CLI output and exit behavior and requires a bounded raw extraction path.
- fixture provenance and benchmark methodology remain evidence requirements before compatibility or performance claims are made.
