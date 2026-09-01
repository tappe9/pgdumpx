use pgdumpx::{
    Archive, ColumnEqualityResult, Compression, EntryReadLimits, FieldRef, OwnedField, OwnedRow,
    ScanLimits,
};
use std::{
    env,
    ffi::{OsStr, OsString},
    fmt,
    fs::File,
    io::{self, BufReader, BufWriter, Write},
    path::{Path, PathBuf},
    process::ExitCode,
};

const NO_MATCH_EXIT: u8 = 1;
const FAILURE_EXIT: u8 = 2;
const DEFAULT_EXTRACT_MAX_DECOMPRESSED_BYTES: u64 = 1_073_741_824;
const USAGE: &str = "usage:\n  pgdumpx inspect <FILE>\n  pgdumpx list <FILE>\n  pgdumpx extract [--max-decompressed-bytes <N>] <FILE> <SCHEMA.TABLE>\n  pgdumpx find [--max-rows <N>] [--max-decompressed-bytes <N>] <FILE> <SCHEMA.TABLE> <COLUMN> <VALUE>\n\nextract raw-entry limit:\n  --max-decompressed-bytes <N> positive maximum decompressed entry bytes\n  omitted limit defaults to 1073741824 bytes (1 GiB)\n\nfind scan limits:\n  --max-rows <N>               positive maximum complete rows evaluated\n  --max-decompressed-bytes <N> positive maximum parser-consumed COPY bytes\n  omitted find limits are unlimited";

fn main() -> ExitCode {
    let stdout = io::stdout();
    let mut stdout = BufWriter::new(stdout.lock());

    match run(env::args_os(), &mut stdout) {
        Ok(CliOutcome::Success) => ExitCode::SUCCESS,
        Ok(CliOutcome::NoMatch) => ExitCode::from(NO_MATCH_EXIT),
        Err(error) => {
            eprintln!("pgdumpx: {error}");
            ExitCode::from(FAILURE_EXIT)
        }
    }
}

fn run<I, W>(arguments: I, stdout: &mut W) -> Result<CliOutcome, CliError>
where
    I: IntoIterator<Item = OsString>,
    W: Write,
{
    match Command::parse(arguments)? {
        Command::Inspect { file } => {
            let archive = open_archive(&file)?;
            write_inspect(stdout, &archive)?;
            flush_stdout(stdout)?;
            Ok(CliOutcome::Success)
        }
        Command::List { file } => {
            let archive = open_archive(&file)?;
            write_list(stdout, &archive)?;
            flush_stdout(stdout)?;
            Ok(CliOutcome::Success)
        }
        Command::Extract(arguments) => {
            let result = extract(&arguments, stdout);
            flush_stdout(stdout)?;
            result?;
            Ok(CliOutcome::Success)
        }
        Command::Find(arguments) => {
            let matched = find(&arguments)?;
            let Some(row) = matched else {
                return Ok(CliOutcome::NoMatch);
            };

            write_row(stdout, &row)
                .map_err(|source| CliError::runtime(format!("stdout error: {source}")))?;
            flush_stdout(stdout)?;
            Ok(CliOutcome::Success)
        }
    }
}

fn open_archive(path: &Path) -> Result<Archive<BufReader<File>>, CliError> {
    let file = File::open(path).map_err(|source| {
        CliError::runtime(format!(
            "failed to open archive {}: {source}",
            path.display()
        ))
    })?;
    Archive::open(BufReader::new(file))
        .map_err(|source| CliError::runtime(format!("archive error: {source}")))
}

fn write_inspect<W: Write, R>(output: &mut W, archive: &Archive<R>) -> Result<(), CliError> {
    let header = archive.header();
    let version = header.version();
    let mut tables = 0_usize;
    let mut table_data = 0_usize;
    for entry in archive.entries() {
        match entry.description_bytes() {
            b"TABLE" => tables += 1,
            b"TABLE DATA" => table_data += 1,
            _ => {}
        }
    }

    writeln!(
        output,
        "archive_version={}.{}.{}",
        version.major(),
        version.minor(),
        version.revision()
    )
    .and_then(|()| {
        writeln!(
            output,
            "compression={}",
            compression_name(header.compression())
        )
    })
    .and_then(|()| writeln!(output, "entries={}", archive.entries().len()))
    .and_then(|()| writeln!(output, "tables={tables}"))
    .and_then(|()| writeln!(output, "table_data={table_data}"))
    .map_err(|source| CliError::runtime(format!("stdout error: {source}")))
}

fn write_list<W: Write, R>(output: &mut W, archive: &Archive<R>) -> Result<(), CliError> {
    output
        .write_all(b"dump_id\tobject_type\tschema\tname\n")
        .map_err(|source| CliError::runtime(format!("stdout error: {source}")))?;

    for entry in archive.entries() {
        write!(output, "{}\t", entry.id().as_i32())
            .map_err(|source| CliError::runtime(format!("stdout error: {source}")))?;
        write_metadata_bytes(output, entry.description_bytes())?;
        output
            .write_all(b"\t")
            .map_err(|source| CliError::runtime(format!("stdout error: {source}")))?;
        match entry.namespace_bytes() {
            Some(schema) => write_metadata_bytes(output, schema)?,
            None => output
                .write_all(b"-")
                .map_err(|source| CliError::runtime(format!("stdout error: {source}")))?,
        }
        output
            .write_all(b"\t")
            .map_err(|source| CliError::runtime(format!("stdout error: {source}")))?;
        write_metadata_bytes(output, entry.name_bytes())?;
        output
            .write_all(b"\n")
            .map_err(|source| CliError::runtime(format!("stdout error: {source}")))?;
    }
    Ok(())
}

fn compression_name(compression: Compression) -> &'static str {
    match compression {
        Compression::None => "none",
        Compression::Gzip => "gzip",
        Compression::Lz4 => "lz4",
        Compression::Zstd => "zstd",
        _ => "unknown",
    }
}

fn write_metadata_bytes<W: Write>(output: &mut W, bytes: &[u8]) -> Result<(), CliError> {
    write_escaped_bytes(output, bytes)
        .map_err(|source| CliError::runtime(format!("stdout error: {source}")))
}

fn flush_stdout<W: Write>(stdout: &mut W) -> Result<(), CliError> {
    stdout
        .flush()
        .map_err(|source| CliError::runtime(format!("stdout error: {source}")))
}

fn extract<W: Write>(arguments: &ExtractArguments, stdout: &mut W) -> Result<(), CliError> {
    let mut archive = open_archive(&arguments.file)?;
    let table = archive
        .table(arguments.schema.as_bytes(), arguments.table.as_bytes())
        .ok_or_else(|| {
            CliError::runtime("archive error: requested table was not found".to_owned())
        })?;
    let table_id = table.table_entry_id();
    let data_id = table.data_entry_id().ok_or_else(|| {
        CliError::runtime(format!(
            "archive error: TABLE dump ID {} has no related TABLE DATA entry",
            table_id.as_i32()
        ))
    })?;
    let limits =
        EntryReadLimits::unlimited().with_max_decompressed_bytes(arguments.max_decompressed_bytes);

    archive
        .copy_entry_to(data_id, stdout, limits)
        .map(|_| ())
        .map_err(|source| CliError::runtime(format!("archive error: {source}")))
}

fn find(arguments: &FindArguments) -> Result<Option<OwnedRow>, CliError> {
    let file = File::open(&arguments.file).map_err(|source| {
        CliError::runtime(format!(
            "failed to open archive {}: {source}",
            arguments.file.display()
        ))
    })?;
    let mut archive = Archive::open(BufReader::new(file))
        .map_err(|source| CliError::runtime(format!("archive error: {source}")))?;
    let mut rows = archive
        .table_rows(arguments.schema.as_bytes(), arguments.table.as_bytes())
        .map_err(|source| CliError::runtime(format!("archive error: {source}")))?;

    let result = rows
        .find_first_equal_with_limits(
            arguments.scan_limits,
            arguments.column.as_bytes(),
            FieldRef::Bytes(arguments.value.as_bytes()),
        )
        .map_err(|source| CliError::runtime(format!("archive error: {source}")))?;

    match result {
        ColumnEqualityResult::Match(row) => Ok(Some(row)),
        ColumnEqualityResult::NoMatch => Ok(None),
        ColumnEqualityResult::ColumnNotFound => Err(CliError::runtime(format!(
            "column {:?} was not found in {}.{}",
            arguments.column, arguments.schema, arguments.table
        ))),
    }
}

fn write_row<W: Write>(output: &mut W, row: &OwnedRow) -> io::Result<()> {
    for (index, field) in row.fields().iter().enumerate() {
        if index != 0 {
            output.write_all(b"\t")?;
        }
        match field {
            OwnedField::Null => output.write_all(br"\N")?,
            OwnedField::Bytes(bytes) => write_field_bytes(output, bytes)?,
        }
    }
    output.write_all(b"\n")
}

fn write_field_bytes<W: Write>(output: &mut W, bytes: &[u8]) -> io::Result<()> {
    write_escaped_bytes(output, bytes)
}

fn write_escaped_bytes<W: Write>(output: &mut W, bytes: &[u8]) -> io::Result<()> {
    let mut plain_start = 0;
    for (index, &byte) in bytes.iter().enumerate() {
        if matches!(byte, 0x20..=0x7e) && byte != b'\\' {
            continue;
        }

        if plain_start != index {
            output.write_all(&bytes[plain_start..index])?;
        }
        match byte {
            b'\\' => output.write_all(br"\\")?,
            b'\t' => output.write_all(br"\t")?,
            b'\n' => output.write_all(br"\n")?,
            b'\r' => output.write_all(br"\r")?,
            _ => {
                let escaped = [
                    b'\\',
                    b'0' + ((byte >> 6) & 0x07),
                    b'0' + ((byte >> 3) & 0x07),
                    b'0' + (byte & 0x07),
                ];
                output.write_all(&escaped)?;
            }
        }
        plain_start = index + 1;
    }

    if plain_start != bytes.len() {
        output.write_all(&bytes[plain_start..])?;
    }
    Ok(())
}

#[derive(Debug)]
enum Command {
    Inspect { file: PathBuf },
    List { file: PathBuf },
    Extract(ExtractArguments),
    Find(FindArguments),
}

impl Command {
    fn parse<I>(arguments: I) -> Result<Self, CliError>
    where
        I: IntoIterator<Item = OsString>,
    {
        let mut arguments = arguments.into_iter();
        let _program = arguments.next();
        let command = required_utf8(arguments.next(), "command")?;
        match command.as_str() {
            "inspect" => Ok(Self::Inspect {
                file: parse_metadata_file(arguments)?,
            }),
            "list" => Ok(Self::List {
                file: parse_metadata_file(arguments)?,
            }),
            "extract" => Ok(Self::Extract(ExtractArguments::parse_remaining(arguments)?)),
            "find" => Ok(Self::Find(FindArguments::parse_remaining(arguments)?)),
            _ => Err(CliError::usage(format!("unsupported command {command:?}"))),
        }
    }
}

fn parse_metadata_file<I>(mut arguments: I) -> Result<PathBuf, CliError>
where
    I: Iterator<Item = OsString>,
{
    let file = arguments
        .next()
        .map(PathBuf::from)
        .ok_or_else(|| CliError::usage("FILE is required"))?;
    if arguments.next().is_some() {
        return Err(CliError::usage("too many arguments"));
    }
    Ok(file)
}

#[derive(Debug)]
struct ExtractArguments {
    file: PathBuf,
    schema: String,
    table: String,
    max_decompressed_bytes: u64,
}

impl ExtractArguments {
    fn parse_remaining<I>(mut arguments: I) -> Result<Self, CliError>
    where
        I: Iterator<Item = OsString>,
    {
        let mut max_decompressed_bytes = None;

        let file = loop {
            let argument = arguments
                .next()
                .ok_or_else(|| CliError::usage("FILE is required"))?;

            if argument.as_os_str() == OsStr::new("--max-decompressed-bytes") {
                let value = parse_positive_limit(&mut arguments, "--max-decompressed-bytes")?;
                if max_decompressed_bytes.replace(value).is_some() {
                    return Err(CliError::usage(
                        "--max-decompressed-bytes may be specified only once",
                    ));
                }
                continue;
            }

            if argument.as_os_str() == OsStr::new("--") {
                break arguments
                    .next()
                    .map(PathBuf::from)
                    .ok_or_else(|| CliError::usage("FILE is required after --"))?;
            }

            if argument
                .to_str()
                .is_some_and(|value| value.starts_with("--"))
            {
                return Err(CliError::usage(format!(
                    "unsupported extract option {argument:?}"
                )));
            }

            break PathBuf::from(argument);
        };

        let table_selector = required_utf8(arguments.next(), "SCHEMA.TABLE")?;
        if arguments.next().is_some() {
            return Err(CliError::usage("too many arguments"));
        }
        let (schema, table) = parse_table_selector(&table_selector)?;

        Ok(Self {
            file,
            schema: schema.to_owned(),
            table: table.to_owned(),
            max_decompressed_bytes: max_decompressed_bytes
                .unwrap_or(DEFAULT_EXTRACT_MAX_DECOMPRESSED_BYTES),
        })
    }
}

#[derive(Debug)]
struct FindArguments {
    file: PathBuf,
    schema: String,
    table: String,
    column: String,
    value: String,
    scan_limits: ScanLimits,
}

impl FindArguments {
    fn parse_remaining<I>(mut arguments: I) -> Result<Self, CliError>
    where
        I: Iterator<Item = OsString>,
    {
        let mut max_rows = None;
        let mut max_decompressed_bytes = None;

        let file = loop {
            let argument = arguments
                .next()
                .ok_or_else(|| CliError::usage("FILE is required"))?;

            if argument.as_os_str() == OsStr::new("--max-rows") {
                let value = parse_positive_limit(&mut arguments, "--max-rows")?;
                if max_rows.replace(value).is_some() {
                    return Err(CliError::usage("--max-rows may be specified only once"));
                }
                continue;
            }

            if argument.as_os_str() == OsStr::new("--max-decompressed-bytes") {
                let value = parse_positive_limit(&mut arguments, "--max-decompressed-bytes")?;
                if max_decompressed_bytes.replace(value).is_some() {
                    return Err(CliError::usage(
                        "--max-decompressed-bytes may be specified only once",
                    ));
                }
                continue;
            }

            if argument.as_os_str() == OsStr::new("--") {
                break arguments
                    .next()
                    .map(PathBuf::from)
                    .ok_or_else(|| CliError::usage("FILE is required after --"))?;
            }

            if argument
                .to_str()
                .is_some_and(|value| value.starts_with("--"))
            {
                return Err(CliError::usage(format!(
                    "unsupported find option {argument:?}"
                )));
            }

            break PathBuf::from(argument);
        };

        let table_selector = required_utf8(arguments.next(), "SCHEMA.TABLE")?;
        let column = required_utf8(arguments.next(), "COLUMN")?;
        let value = required_utf8(arguments.next(), "VALUE")?;
        if arguments.next().is_some() {
            return Err(CliError::usage("too many arguments"));
        }
        if column.is_empty() {
            return Err(CliError::usage("COLUMN must not be empty"));
        }

        let (schema, table) = parse_table_selector(&table_selector)?;
        let mut scan_limits = ScanLimits::unlimited();
        if let Some(value) = max_rows {
            scan_limits = scan_limits.with_max_rows(value);
        }
        if let Some(value) = max_decompressed_bytes {
            scan_limits = scan_limits.with_max_decompressed_bytes(value);
        }

        Ok(Self {
            file,
            schema: schema.to_owned(),
            table: table.to_owned(),
            column,
            value,
            scan_limits,
        })
    }
}

fn parse_positive_limit<I>(arguments: &mut I, option: &str) -> Result<u64, CliError>
where
    I: Iterator<Item = OsString>,
{
    let value = required_utf8(arguments.next(), option)?;
    let parsed = value
        .parse::<u64>()
        .map_err(|_| CliError::usage(format!("{option} must be a positive u64 integer")))?;
    if parsed == 0 {
        return Err(CliError::usage(format!(
            "{option} must be greater than zero"
        )));
    }
    Ok(parsed)
}

fn required_utf8(argument: Option<OsString>, name: &str) -> Result<String, CliError> {
    let argument = argument.ok_or_else(|| CliError::usage(format!("{name} is required")))?;
    argument
        .into_string()
        .map_err(|_| CliError::usage(format!("{name} must be valid UTF-8")))
}

fn parse_table_selector(selector: &str) -> Result<(&str, &str), CliError> {
    if selector.contains('"') {
        return Err(CliError::usage(
            "SCHEMA.TABLE does not support SQL identifier quoting",
        ));
    }

    let mut components = selector.split('.');
    let schema = components.next().unwrap_or_default();
    let table = components.next().unwrap_or_default();
    if schema.is_empty() || table.is_empty() || components.next().is_some() {
        return Err(CliError::usage(
            "SCHEMA.TABLE must contain exactly one ASCII '.' separator and non-empty components",
        ));
    }
    Ok((schema, table))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CliOutcome {
    Success,
    NoMatch,
}

#[derive(Debug)]
struct CliError {
    message: String,
}

impl CliError {
    fn usage(message: impl fmt::Display) -> Self {
        Self {
            message: format!("{message}\n{USAGE}"),
        }
    }

    fn runtime(message: String) -> Self {
        Self { message }
    }
}

impl fmt::Display for CliError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

#[cfg(test)]
mod tests {
    use super::{FindArguments, write_inspect, write_list};
    use pgdumpx::Archive;
    use std::{
        cell::Cell,
        ffi::OsString,
        io::{self, Cursor, Read, Seek, SeekFrom},
        path::PathBuf,
        rc::Rc,
    };

    #[test]
    fn find_defaults_are_finite_and_each_override_preserves_the_other_default() {
        const DEFAULT_ROWS: u64 = 100_000;
        const DEFAULT_BYTES: u64 = 64 * 1024 * 1024;

        let defaults = parse_find(&[]).unwrap();
        assert_eq!(defaults.scan_limits.max_rows(), Some(DEFAULT_ROWS));
        assert_eq!(
            defaults.scan_limits.max_decompressed_bytes(),
            Some(DEFAULT_BYTES)
        );

        let rows_only = parse_find(&["--max-rows", "5"]).unwrap();
        assert_eq!(rows_only.scan_limits.max_rows(), Some(5));
        assert_eq!(
            rows_only.scan_limits.max_decompressed_bytes(),
            Some(DEFAULT_BYTES)
        );

        let bytes_only = parse_find(&["--max-decompressed-bytes", "512"]).unwrap();
        assert_eq!(bytes_only.scan_limits.max_rows(), Some(DEFAULT_ROWS));
        assert_eq!(bytes_only.scan_limits.max_decompressed_bytes(), Some(512));

        let both = parse_find(&["--max-rows", "7", "--max-decompressed-bytes", "1024"]).unwrap();
        assert_eq!(both.scan_limits.max_rows(), Some(7));
        assert_eq!(both.scan_limits.max_decompressed_bytes(), Some(1024));
    }

    #[test]
    fn unlimited_find_is_explicit_and_conflicts_with_finite_options() {
        let unlimited = parse_find(&["--unlimited"]).unwrap();
        assert_eq!(unlimited.scan_limits.max_rows(), None);
        assert_eq!(unlimited.scan_limits.max_decompressed_bytes(), None);

        for options in [
            &["--unlimited", "--max-rows", "1"][..],
            &["--max-rows", "1", "--unlimited"][..],
            &["--unlimited", "--max-decompressed-bytes", "1"][..],
        ] {
            let error = parse_find(options).unwrap_err().to_string();
            assert!(error.contains("cannot be combined"), "error={error:?}");
        }

        let duplicate = parse_find(&["--unlimited", "--unlimited"])
            .unwrap_err()
            .to_string();
        assert!(duplicate.contains("may be specified only once"));
    }

    fn parse_find(options: &[&str]) -> Result<FindArguments, super::CliError> {
        let arguments = options
            .iter()
            .copied()
            .chain(["archive.dump", "public.orders", "order_number", "value"])
            .map(OsString::from);
        FindArguments::parse_remaining(arguments)
    }

    #[test]
    fn metadata_rendering_never_reads_or_seeks_after_archive_open() {
        for fixture_name in ["pg18-none-copy-basic.dump", "pg18-gzip-copy-basic.dump"] {
            let bytes = std::fs::read(fixture_path(fixture_name)).unwrap();
            let reads = Rc::new(Cell::new(0_u64));
            let seeks = Rc::new(Cell::new(0_u64));
            let reader = TrackingReader::new(bytes, Rc::clone(&reads), Rc::clone(&seeks));
            let archive = Archive::open(reader).unwrap();
            let after_open_reads = reads.get();
            let after_open_seeks = seeks.get();

            let mut inspect = Vec::new();
            write_inspect(&mut inspect, &archive).unwrap();
            let mut list = Vec::new();
            write_list(&mut list, &archive).unwrap();

            assert_eq!(reads.get(), after_open_reads, "fixture={fixture_name}");
            assert_eq!(seeks.get(), after_open_seeks, "fixture={fixture_name}");
            assert_eq!(after_open_seeks, 0, "metadata open must not seek");
        }
    }

    fn fixture_path(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/archives")
            .join(name)
    }

    struct TrackingReader {
        inner: Cursor<Vec<u8>>,
        reads: Rc<Cell<u64>>,
        seeks: Rc<Cell<u64>>,
    }

    impl TrackingReader {
        fn new(bytes: Vec<u8>, reads: Rc<Cell<u64>>, seeks: Rc<Cell<u64>>) -> Self {
            Self {
                inner: Cursor::new(bytes),
                reads,
                seeks,
            }
        }
    }

    impl Read for TrackingReader {
        fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
            let read = self.inner.read(output)?;
            self.reads
                .set(self.reads.get() + u64::try_from(read).unwrap());
            Ok(read)
        }
    }

    impl Seek for TrackingReader {
        fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
            self.seeks.set(self.seeks.get() + 1);
            self.inner.seek(position)
        }
    }
}
