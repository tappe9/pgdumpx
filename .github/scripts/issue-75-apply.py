from pathlib import Path


def replace_exact(text: str, old: str, new: str, *, path: Path, count: int = 1) -> str:
    actual = text.count(old)
    if actual != count:
        raise SystemExit(
            f"{path}: expected {count} occurrence(s) of {old[:100]!r}, found {actual}"
        )
    return text.replace(old, new, count)


def update_main() -> None:
    path = Path("crates/pgdumpx-cli/src/main.rs")
    text = path.read_text()

    old = """const NO_MATCH_EXIT: u8 = 1;
const FAILURE_EXIT: u8 = 2;
const DEFAULT_EXTRACT_MAX_DECOMPRESSED_BYTES: u64 = 1_073_741_824;
const USAGE: &str = \"usage:\\n  pgdumpx inspect <FILE>\\n  pgdumpx list <FILE>\\n  pgdumpx extract [--max-decompressed-bytes <N>] <FILE> <SCHEMA.TABLE>\\n  pgdumpx find [--max-rows <N>] [--max-decompressed-bytes <N>] <FILE> <SCHEMA.TABLE> <COLUMN> <VALUE>\\n\\nextract raw-entry limit:\\n  --max-decompressed-bytes <N> positive maximum decompressed entry bytes\\n  omitted limit defaults to 1073741824 bytes (1 GiB)\\n\\nfind scan limits:\\n  --max-rows <N>               positive maximum complete rows evaluated\\n  --max-decompressed-bytes <N> positive maximum parser-consumed COPY bytes\\n  omitted find limits are unlimited\";
"""
    new = """const NO_MATCH_EXIT: u8 = 1;
const FAILURE_EXIT: u8 = 2;
const DEFAULT_EXTRACT_MAX_DECOMPRESSED_BYTES: u64 = 1_073_741_824;
const DEFAULT_FIND_MAX_ROWS: u64 = 100_000;
const DEFAULT_FIND_MAX_DECOMPRESSED_BYTES: u64 = 64 * 1024 * 1024;
const USAGE: &str = \"usage:\\n  pgdumpx inspect <FILE>\\n  pgdumpx list <FILE>\\n  pgdumpx extract [--max-decompressed-bytes <N>] <FILE> <SCHEMA.TABLE>\\n  pgdumpx find [--unlimited | [--max-rows <N>] [--max-decompressed-bytes <N>]] <FILE> <SCHEMA.TABLE> <COLUMN> <VALUE>\\n\\nextract raw-entry limit:\\n  --max-decompressed-bytes <N> positive maximum decompressed entry bytes\\n  omitted limit defaults to 1073741824 bytes (1 GiB)\\n\\nfind scan limits:\\n  --max-rows <N>               positive maximum complete rows evaluated\\n  omitted row limit defaults to 100000 complete rows\\n  --max-decompressed-bytes <N> positive maximum parser-consumed COPY bytes\\n  omitted byte limit defaults to 67108864 bytes (64 MiB)\\n  --unlimited                  intentionally disable both total scan budgets\";
"""
    text = replace_exact(text, old, new, path=path)

    old = """        Command::Find(arguments) => {
            let matched = find(&arguments)?;
"""
    new = """        Command::Help => {
            stdout
                .write_all(USAGE.as_bytes())
                .and_then(|()| stdout.write_all(b\"\\n\"))
                .map_err(|source| CliError::runtime(format!(\"stdout error: {source}\")))?;
            flush_stdout(stdout)?;
            Ok(CliOutcome::Success)
        }
        Command::Find(arguments) => {
            let matched = find(&arguments)?;
"""
    text = replace_exact(text, old, new, path=path)

    old = """enum Command {
    Inspect { file: PathBuf },
    List { file: PathBuf },
    Extract(ExtractArguments),
    Find(FindArguments),
}
"""
    new = """enum Command {
    Inspect { file: PathBuf },
    List { file: PathBuf },
    Extract(ExtractArguments),
    Help,
    Find(FindArguments),
}
"""
    text = replace_exact(text, old, new, path=path)

    old = """            \"extract\" => Ok(Self::Extract(ExtractArguments::parse_remaining(arguments)?)),
            \"find\" => Ok(Self::Find(FindArguments::parse_remaining(arguments)?)),
"""
    new = """            \"extract\" => Ok(Self::Extract(ExtractArguments::parse_remaining(arguments)?)),
            \"find\" => {
                let remaining = arguments.collect::<Vec<_>>();
                if remaining.len() == 1
                    && matches!(remaining[0].to_str(), Some(\"--help\" | \"-h\"))
                {
                    Ok(Self::Help)
                } else {
                    Ok(Self::Find(FindArguments::parse_remaining(
                        remaining.into_iter(),
                    )?))
                }
            }
"""
    text = replace_exact(text, old, new, path=path)

    old = """        let mut max_rows = None;
        let mut max_decompressed_bytes = None;
"""
    new = """        let mut max_rows = None;
        let mut max_decompressed_bytes = None;
        let mut unlimited = false;
"""
    text = replace_exact(text, old, new, path=path)

    old = """            if argument.as_os_str() == OsStr::new(\"--max-rows\") {
"""
    new = """            if argument.as_os_str() == OsStr::new(\"--unlimited\") {
                if unlimited {
                    return Err(CliError::usage(
                        \"--unlimited may be specified only once\",
                    ));
                }
                unlimited = true;
                continue;
            }

            if argument.as_os_str() == OsStr::new(\"--max-rows\") {
"""
    text = replace_exact(text, old, new, path=path)

    old = """        let (schema, table) = parse_table_selector(&table_selector)?;
        let mut scan_limits = ScanLimits::unlimited();
        if let Some(value) = max_rows {
            scan_limits = scan_limits.with_max_rows(value);
        }
        if let Some(value) = max_decompressed_bytes {
            scan_limits = scan_limits.with_max_decompressed_bytes(value);
        }
"""
    new = """        let (schema, table) = parse_table_selector(&table_selector)?;
        if unlimited && (max_rows.is_some() || max_decompressed_bytes.is_some()) {
            return Err(CliError::usage(
                \"--unlimited cannot be combined with --max-rows or --max-decompressed-bytes\",
            ));
        }
        let scan_limits = if unlimited {
            ScanLimits::unlimited()
        } else {
            ScanLimits::unlimited()
                .with_max_rows(max_rows.unwrap_or(DEFAULT_FIND_MAX_ROWS))
                .with_max_decompressed_bytes(
                    max_decompressed_bytes.unwrap_or(DEFAULT_FIND_MAX_DECOMPRESSED_BYTES),
                )
        };
"""
    text = replace_exact(text, old, new, path=path)
    path.write_text(text)


def update_readme() -> None:
    path = Path("README.md")
    text = path.read_text()

    text = replace_exact(
        text,
        """pgdumpx find [--max-rows <N>] [--max-decompressed-bytes <N>] \\
  <FILE> <SCHEMA.TABLE> <COLUMN> <VALUE>
""",
        """pgdumpx find [--unlimited | [--max-rows <N>] [--max-decompressed-bytes <N>]] \\
  <FILE> <SCHEMA.TABLE> <COLUMN> <VALUE>
""",
        path=path,
    )
    text = replace_exact(
        text,
        "Each optional scan-limit flag accepts a positive decimal `u64`, may be specified at most once, and appears before `<FILE>`. Omitting a flag leaves that budget unlimited. `--max-rows <N>` counts complete rows evaluated by the library search path, including the matching row. `--max-decompressed-bytes <N>` uses parser-consumed physical COPY-byte accounting; it includes separators, row terminators, escape spellings, and a consumed COPY terminator, but excludes unread decompressor/buffer lookahead and decoded logical-length changes.\n",
        "Each optional finite scan-limit flag accepts a positive decimal `u64`, may be specified at most once, and appears before `<FILE>`. Without options, `find` applies inclusive defaults of **100,000 complete rows** and **67,108,864 parser-consumed decompressed bytes (64 MiB)**. Supplying only one finite option overrides only that dimension and leaves the other finite default in force. Trusted workflows may pass `--unlimited` to disable both total-work budgets explicitly; it is mutually exclusive with either finite option. `--max-rows <N>` counts complete rows evaluated by the library search path, including the matching row. `--max-decompressed-bytes <N>` uses parser-consumed physical COPY-byte accounting; it includes separators, row terminators, escape spellings, and a consumed COPY terminator, but excludes unread decompressor/buffer lookahead and decoded logical-length changes. See [the `find` scan-budget policy](docs/FIND-SCAN-LIMITS.md) for selection evidence, exact boundaries, and migration guidance.\n",
        path=path,
    )
    text = replace_exact(
        text,
        "It writes a diagnostic to stderr and exits with `2+` even when no matching row was reached before exhaustion.",
        "It writes a diagnostic to stderr and exits with `2` even when no matching row was reached before exhaustion.",
        path=path,
    )
    text = replace_exact(
        text,
        "2+ usage, I/O, format, integrity, decompression, COPY, encoding,",
        "2  usage, I/O, format, integrity, decompression, COPY, encoding,",
        path=path,
    )
    marker = "- [Bounded raw extraction](docs/RAW-EXTRACTION.md) — raw byte-budget and partial-output semantics;\n"
    text = replace_exact(
        text,
        marker,
        marker
        + "- [`find` scan-budget policy](docs/FIND-SCAN-LIMITS.md) — finite CLI defaults, evidence, boundary semantics, and migration guidance;\n",
        path=path,
    )
    path.write_text(text)


def update_readme_ja() -> None:
    path = Path("README.ja.md")
    text = path.read_text()

    text = replace_exact(
        text,
        """pgdumpx find [--max-rows <N>] [--max-decompressed-bytes <N>] \\
  <FILE> <SCHEMA.TABLE> <COLUMN> <VALUE>
""",
        """pgdumpx find [--unlimited | [--max-rows <N>] [--max-decompressed-bytes <N>]] \\
  <FILE> <SCHEMA.TABLE> <COLUMN> <VALUE>
""",
        path=path,
    )
    text = replace_exact(
        text,
        "optionalなscan-limit flagはそれぞれ正の10進`u64`を1回まで指定でき、`<FILE>`より前に置きます。省略したbudgetはunlimitedです。`--max-rows <N>`はlibrary search pathがevaluateしたcomplete rowを数え、一致rowも含みます。`--max-decompressed-bytes <N>`はparser-consumed physical COPY-byte会計を使い、separator、row terminator、escape spelling、消費したCOPY terminatorを含みますが、decompressor / bufferの未消費lookaheadやlogical decodeによるlength変化は含みません。\n",
        "optionalなfinite scan-limit flagはそれぞれ正の10進`u64`を1回まで指定でき、`<FILE>`より前に置きます。option未指定時は、**complete row 100,000件**と**parser-consumed decompressed bytes 67,108,864 bytes (64 MiB)**のinclusiveなdefaultを両方適用します。finite optionを片方だけ指定した場合はその次元だけを上書きし、もう一方のfinite defaultは残ります。trusted workflowでは`--unlimited`により両方のtotal-work budgetを明示的に解除できますが、finite optionとは排他的です。`--max-rows <N>`はlibrary search pathがevaluateしたcomplete rowを数え、一致rowも含みます。`--max-decompressed-bytes <N>`はparser-consumed physical COPY-byte会計を使い、separator、row terminator、escape spelling、消費したCOPY terminatorを含みますが、decompressor / bufferの未消費lookaheadやlogical decodeによるlength変化は含みません。数値の根拠、exact boundary、移行手順は[`find` scan-budget policy](docs/FIND-SCAN-LIMITS.md)を参照してください。\n",
        path=path,
    )
    text = replace_exact(
        text,
        "stderrへdiagnosticを出し、`2+`で終了します。",
        "stderrへdiagnosticを出し、`2`で終了します。",
        path=path,
    )
    text = replace_exact(
        text,
        "2+ usage / I/O / format / integrity / decompression / COPY / encoding /",
        "2  usage / I/O / format / integrity / decompression / COPY / encoding /",
        path=path,
    )
    marker = "- [Bounded raw extraction](docs/RAW-EXTRACTION.md) — raw byte-budget / partial-output semantics\n"
    text = replace_exact(
        text,
        marker,
        marker
        + "- [`find` scan-budget policy](docs/FIND-SCAN-LIMITS.md) — finite CLI default、根拠、boundary、migration guidance\n",
        path=path,
    )
    path.write_text(text)


def update_security() -> None:
    path = Path("SECURITY.md")
    text = path.read_text()
    old = """Applications processing trusted local backups may choose generous limits. Applications processing untrusted or customer-supplied archives should configure limits appropriate to their service-level and resource constraints.

A low-level unlimited `EntryDataReader` remains available for trusted callers. The library also provides `entry_reader_with_limits` and `copy_entry_to` bounded raw-extraction paths, and `pgdumpx extract` uses the bounded high-level path with a finite 1 GiB default rather than relying on CLI callers to wrap stdout copying themselves.
"""
    new = """Applications processing trusted local backups may choose generous limits. Applications processing untrusted or customer-supplied archives should configure limits appropriate to their service-level and resource constraints.

`pgdumpx find` applies finite defaults of 100,000 complete rows and 64 MiB of parser-consumed decompressed COPY bytes. A one-dimensional override preserves the other finite default. `--unlimited` is an explicit trusted-input opt-in that disables both total-work budgets and must not be used for unreviewed customer-supplied archives merely to bypass a resource error. See [`find` scan-budget policy](docs/FIND-SCAN-LIMITS.md).

A low-level unlimited `EntryDataReader` remains available for trusted callers. The library also provides `entry_reader_with_limits` and `copy_entry_to` bounded raw-extraction paths, and `pgdumpx extract` uses the bounded high-level path with a finite 1 GiB default rather than relying on CLI callers to wrap stdout copying themselves.
"""
    path.write_text(replace_exact(text, old, new, path=path))


def update_roadmap() -> None:
    path = Path("ROADMAP.md")
    text = path.read_text()

    text = replace_exact(
        text,
        "pgdumpx find [--max-rows <N>] [--max-decompressed-bytes <N>] <FILE> <SCHEMA.TABLE> <COLUMN> <VALUE>\n",
        "pgdumpx find [--unlimited | [--max-rows <N>] [--max-decompressed-bytes <N>]] <FILE> <SCHEMA.TABLE> <COLUMN> <VALUE>\n",
        path=path,
    )
    text = replace_exact(
        text,
        """pgdumpx find [--max-rows <N>] [--max-decompressed-bytes <N>] \\
  <FILE> <SCHEMA.TABLE> <COLUMN> <VALUE>
""",
        """pgdumpx find [--unlimited | [--max-rows <N>] [--max-decompressed-bytes <N>]] \\
  <FILE> <SCHEMA.TABLE> <COLUMN> <VALUE>
""",
        path=path,
    )
    old = """- `pgdumpx find` scan options:

```text
--max-rows <N>
--max-decompressed-bytes <N>
```

- bounded raw entry extraction with `EntryReadLimits`;
"""
    new = """- `pgdumpx find` scan options and finite CLI defaults:

```text
--max-rows <N>               default: 100000 complete rows
--max-decompressed-bytes <N> default: 67108864 parser-consumed bytes
--unlimited                  explicit trusted-input opt-in
```

- bounded raw entry extraction with `EntryReadLimits`;
"""
    text = replace_exact(text, old, new, path=path)
    text = replace_exact(
        text,
        "Uses UTF-8 command-line arguments, resolves the column through recorded COPY metadata, and compares the supplied value bytes with logical post-unescape field bytes. Scan-budget options delegate to the same library `ScanLimits` accounting path used by Rust callers. A future byte-literal input mode requires a separate CLI design.\n",
        "Uses UTF-8 command-line arguments, resolves the column through recorded COPY metadata, and compares the supplied value bytes with logical post-unescape field bytes. The CLI applies finite defaults of 100,000 complete rows and 64 MiB of parser-consumed bytes; one finite override preserves the other default, while `--unlimited` explicitly disables both. Scan-budget options delegate to the same library `ScanLimits` accounting path used by Rust callers. A future byte-literal input mode requires a separate CLI design.\n",
        path=path,
    )
    text = replace_exact(
        text,
        "2+ usage, I/O, format, integrity, decompression, COPY, encoding,",
        "2  usage, I/O, format, integrity, decompression, COPY, encoding,",
        path=path,
    )
    path.write_text(text)


def main() -> None:
    update_main()
    update_readme()
    update_readme_ja()
    update_security()
    update_roadmap()


if __name__ == "__main__":
    main()
