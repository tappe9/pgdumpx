# `pgdumpx find` scan-budget policy

`pgdumpx find` performs a streaming sequential scan of the selected table-data entry. It is not backed by a row-level index, so an absent or late match can otherwise consume the complete decompressed COPY stream.

## Finite CLI defaults

When neither finite option nor `--unlimited` is supplied, the CLI applies both inclusive budgets:

| Dimension | Default |
| --- | ---: |
| Complete rows evaluated | 100,000 |
| Parser-consumed decompressed COPY bytes | 67,108,864 bytes (64 MiB) |

The Rust library contract is unchanged: `ScanLimits::default()` and `ScanLimits::unlimited()` remain unlimited. These values are a CLI safety policy for `find` only. The raw `extract` command keeps its separate `EntryReadLimits` policy and 1 GiB default.

A caller can override one finite dimension without disabling the other default:

```bash
pgdumpx find --max-rows 250000 backup.dump public.orders order_number 123456
pgdumpx find --max-decompressed-bytes 134217728 \
  backup.dump public.orders order_number 123456
```

Trusted workflows that intentionally accept an unbounded sequential scan must opt in explicitly:

```bash
pgdumpx find --unlimited backup.dump public.orders order_number 123456
```

`--unlimited` is mutually exclusive with `--max-rows` and `--max-decompressed-bytes`. Every option may appear at most once. Conflicts, duplicates, zero, negative, non-decimal, and overflowing values are usage errors.

## Selection evidence

The values are based on existing repository limit policy rather than an assumed typical database size:

- 100,000 is already the compatibility-oriented high-cardinality bound used for archive TOC entries and per-entry dependencies.
- 64 MiB is already an established finite repository budget for derived metadata/index names.
- The byte default is large enough to permit normal bounded inspection while still preventing an omitted option from silently becoming unlimited.

The committed PostgreSQL 18 basic COPY fixtures provide a reproducible lower-bound compatibility measurement. For each supported compression backend, a complete no-match scan evaluates 7 rows and consumes exactly 268 physical COPY bytes through the terminator:

| Fixture | Rows | Parser-consumed bytes |
| --- | ---: | ---: |
| `pg18-none-copy-basic.dump` | 7 | 268 |
| `pg18-gzip-copy-basic.dump` | 7 | 268 |
| `pg18-lz4-copy-basic.dump` | 7 | 268 |
| `pg18-zstd-copy-basic.dump` | 7 | 268 |

The integration contract reproduces this by running `find` with `--max-rows 7 --max-decompressed-bytes 268`, which completes as a clean no-match, and then with 267 bytes, which produces a typed resource failure. A generated 100,001-row archive verifies that the default row budget is active when no option is supplied.

## Boundary and exit behavior

Configured finite values are inclusive. A scan may evaluate exactly `N` complete rows or consume exactly `N` parser-consumed bytes. The first row or consumed byte above the configured budget produces a typed resource error before a crossing row is returned or evaluated as a match.

The CLI exit contract is:

```text
0  match found
1  complete scan within budget with no match
2  usage, resource, I/O, format, integrity, decompression, COPY,
   encoding, unsupported representation, or unknown-column failure
```

Budget exhaustion is never reported as a clean no-match. Diagnostics go to stderr and match/no-match output remains on stdout only.

## Migration note

Before this policy, omitting both `find` limit options selected `ScanLimits::unlimited()`. Scripts that deliberately scan more than 100,000 rows or 64 MiB of parser-consumed COPY data must now set appropriate finite overrides or pass `--unlimited` after reviewing the input trust and resource-risk boundary.
