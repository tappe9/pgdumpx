use pgdumpx::{Archive, Compression, FieldRef, OwnedField, OwnedRow, ScanLimits};
use std::{
    env,
    ffi::{OsStr, OsString},
    fmt,
    fs::File,
    io::{self, BufReader, Write},
    path::PathBuf,
    process::ExitCode,
};

const USAGE_EXIT: u8 = 2;
const FAILURE_EXIT: u8 = 2;
const USAGE: &str = "usage:\n  pgdumpx inspect <FILE>\n  pgdumpx list <FILE>\n  pgdumpx find [--max-rows <N>] [--max-decompressed-bytes <N>] <FILE> <SCHEMA.TABLE> <COLUMN> <VALUE>\n\nfind scan limits:\n  --max-rows <N>               positive maximum complete rows evaluated\n  --max-decompressed-bytes <N> positive maximum parser-consumed COPY bytes\n  omitted limits are unlimited";

fn main() -> ExitCode {
    match Command::parse(env::args_os().skip(1)).and_then(Command::execute) {
        Ok(code) => code,
        Err(error) => {
            let _ = writeln!(io::stderr(), "pgdumpx: {error}");
            if error.kind == ErrorKind::Usage {
                let _ = writeln!(io::stderr(), "{USAGE}");
                ExitCode::from(USAGE_EXIT)
            } else {
                ExitCode::from(FAILURE_EXIT)
            }
        }
    }
}

#[derive(Debug)]
enum Command {
    Inspect(PathBuf),
    List(PathBuf),
    Find(FindArguments),
}

impl Command {
    fn parse<I>(mut arguments: I) -> Result<Self, CliError>
    where
        I: Iterator<Item = OsString>,
    {
        let command = arguments
            .next()
            .ok_or_else(|| CliError::usage("missing command"))?;
        let command = command
            .into_string()
            .map_err(|_| CliError::usage("command must be valid UTF-8"))?;

        match command.as_str() {
            "inspect" => Ok(Self::Inspect(parse_single_file(arguments)?)),
            "list" => Ok(Self::List(parse_single_file(arguments)?)),
            "find" => Ok(Self::Find(FindArguments::parse(arguments)?)),
            _ => Err(CliError::usage(format!("unknown command {command:?}"))),
        }
    }

    fn execute(self) -> Result<ExitCode, CliError> {
        match self {
            Self::Inspect(path) => {
                let archive = open_archive(&path)?;
                print_inspect(&archive)?;
                Ok(ExitCode::SUCCESS)
            }
            Self::List(path) => {
                let archive = open_archive(&path)?;
                print_list(&archive)?;
                Ok(ExitCode::SUCCESS)
            }
            Self::Find(arguments) => find(arguments),
        }
    }
}

#[derive(Debug)]
struct FindArguments {
    file: PathBuf,
    schema: Vec<u8>,
    table: Vec<u8>,
    column: Vec<u8>,
    value: Vec<u8>,
    scan_limits: ScanLimits,
}

impl FindArguments {
    fn parse<I>(arguments: I) -> Result<Self, CliError>
    where
        I: Iterator<Item = OsString>,
    {
        Self::parse_remaining(arguments)
    }

    fn parse_remaining<I>(mut arguments: I) -> Result<Self, CliError>
    where
        I: Iterator<Item = OsString>,
    {
        let mut max_rows = None;
        let mut max_decompressed_bytes = None;

        let file = loop {
            let argument = arguments
                .next()
                .ok_or_else(|| CliError::usage("missing FILE argument"))?;

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
                break PathBuf::from(
                    arguments
                        .next()
                        .ok_or_else(|| CliError::usage("missing FILE argument after --"))?,
                );
            }

            if argument
                .to_str()
                .is_some_and(|value| value.starts_with("--"))
            {
                return Err(CliError::usage(format!(
                    "unknown find option {argument:?}"
                )));
            }

            break PathBuf::from(argument);
        };

        let selector = required_utf8_argument(&mut arguments, "SCHEMA.TABLE")?;
        let (schema, table) = parse_table_selector(selector)?;
        let column = required_utf8_argument(&mut arguments, "COLUMN")?.into_bytes();
        let value = required_utf8_argument(&mut arguments, "VALUE")?.into_bytes();
        if arguments.next().is_some() {
            return Err(CliError::usage("too many arguments"));
        }

        let mut scan_limits = ScanLimits::unlimited();
        if let Some(value) = max_rows {
            scan_limits = scan_limits.with_max_rows(value);
        }
        if let Some(value) = max_decompressed_bytes {
            scan_limits = scan_limits.with_max_decompressed_bytes(value);
        }

        Ok(Self {
            file,
            schema,
            table,
            column,
            value,
            scan_limits,
        })
    }
}

fn parse_positive_limit<I>(arguments: &mut I, option: &'static str) -> Result<u64, CliError>
where
    I: Iterator<Item = OsString>,
{
    let value = arguments
        .next()
        .ok_or_else(|| CliError::usage(format!("missing value for {option}")))?;
    let value = value
        .into_string()
        .map_err(|_| CliError::usage(format!("{option} value must be valid UTF-8")))?;
    let parsed = value.parse::<u64>().map_err(|_| {
        CliError::usage(format!(
            "{option} must be a positive integer in the range 1..={} ",
            u64::MAX
        ))
    })?;
    if parsed == 0 {
        return Err(CliError::usage(format!(
            "{option} must be greater than zero"
        )));
    }
    Ok(parsed)
}

fn required_utf8_argument<I>(
    arguments: &mut I,
    name: &'static str,
) -> Result<String, CliError>
where
    I: Iterator<Item = OsString>,
{
    arguments
        .next()
        .ok_or_else(|| CliError::usage(format!("missing {name} argument")))?
        .into_string()
        .map_err(|_| CliError::usage(format!("{name} must be valid UTF-8")))
}

fn parse_table_selector(selector: String) -> Result<(Vec<u8>, Vec<u8>), CliError> {
    let Some((schema, table)) = selector.split_once('.') else {
        return Err(CliError::usage(
            "SCHEMA.TABLE must contain exactly one '.' separator",
        ));
    };
    if schema.is_empty() || table.is_empty() || table.contains('.') {
        return Err(CliError::usage(
            "SCHEMA.TABLE must contain non-empty schema and table names separated by one '.'",
        ));
    }
    if selector.contains('"') {
        return Err(CliError::usage(
            "SCHEMA.TABLE does not accept SQL identifier quoting",
        ));
    }
    Ok((schema.as_bytes().to_vec(), table.as_bytes().to_vec()))
}

fn parse_single_file<I>(mut arguments: I) -> Result<PathBuf, CliError>
where
    I: Iterator<Item = OsString>,
{
    let path = arguments
        .next()
        .ok_or_else(|| CliError::usage("missing FILE argument"))?;
    if arguments.next().is_some() {
        return Err(CliError::usage("too many arguments"));
    }
    Ok(PathBuf::from(path))
}

fn open_archive(path: &PathBuf) -> Result<Archive<BufReader<File>>, CliError> {
    let file = File::open(path)
        .map_err(|source| CliError::runtime(format!("could not open {}: {source}", path.display())))?;
    Archive::open(BufReader::new(file)).map_err(|error| {
        CliError::runtime(format!(
            "could not parse archive {}: {error}",
            path.display()
        ))
    })
}

fn find(arguments: FindArguments) -> Result<ExitCode, CliError> {
    let file = File::open(&arguments.file).map_err(|source| {
        CliError::runtime(format!(
            "could not open {}: {source}",
            arguments.file.display()
        ))
    })?;
    let mut archive = Archive::open(BufReader::new(file)).map_err(|error| {
        CliError::runtime(format!(
            "could not parse archive {}: {error}",
            arguments.file.display()
        ))
    })?;
    let mut rows = archive
        .table_rows(&arguments.schema, &arguments.table)
        .map_err(|error| CliError::runtime(format!("could not open table rows: {error}")))?;
    let column_index = rows
        .column_index(&arguments.column)
        .map_err(|error| CliError::runtime(format!("could not resolve column metadata: {error}")))?
        .ok_or_else(|| {
            CliError::runtime(format!(
                "column {} was not found in {}.{}",
                String::from_utf8_lossy(&arguments.column),
                String::from_utf8_lossy(&arguments.schema),
                String::from_utf8_lossy(&arguments.table)
            ))
        })?;

    let found = rows
        .find_first_with_limits(arguments.scan_limits, |row| {
            row.field(column_index) == Some(FieldRef::Bytes(&arguments.value))
        })
        .map_err(|error| CliError::runtime(format!("row scan failed: {error}")))?;

    match found {
        Some(row) => {
            write_match(io::stdout().lock(), &row)
                .map_err(|source| CliError::runtime(format!("could not write stdout: {source}")))?;
            Ok(ExitCode::SUCCESS)
        }
        None => Ok(ExitCode::from(1)),
    }
}

fn print_inspect<R: std::io::Read>(archive: &Archive<R>) -> Result<(), CliError> {
    let mut output = io::BufWriter::new(io::stdout().lock());
    render_inspect(archive, &mut output)
        .map_err(|source| CliError::runtime(format!("could not write stdout: {source}")))
}

fn print_list<R: std::io::Read>(archive: &Archive<R>) -> Result<(), CliError> {
    let mut output = io::BufWriter::new(io::stdout().lock());
    render_list(archive, &mut output)
        .map_err(|source| CliError::runtime(format!("could not write stdout: {source}")))
}

fn render_inspect<R: std::io::Read, W: Write>(
    archive: &Archive<R>,
    output: &mut W,
) -> io::Result<()> {
    let header = archive.header();
    writeln!(
        output,
        "archive_version\t{}.{}.{}",
        header.version().major(),
        header.version().minor(),
        header.version().revision()
    )?;
    writeln!(
        output,
        "compression\t{}\t{}",
        compression_name(header.compression()),
        header.compression_level()
    )?;
    write!(output, "database\t")?;
    write_optional_archive_string(output, header.database_name())?;
    writeln!(output)?;
    write!(output, "remote_version\t")?;
    write_archive_bytes(output, header.remote_version().as_bytes())?;
    writeln!(output)?;
    write!(output, "dump_version\t")?;
    write_archive_bytes(output, header.dump_version().as_bytes())?;
    writeln!(output)?;
    writeln!(output, "toc_entries\t{}", archive.entries().len())?;
    writeln!(output, "tables\t{}", archive.tables().count())?;
    Ok(())
}

fn render_list<R: std::io::Read, W: Write>(
    archive: &Archive<R>,
    output: &mut W,
) -> io::Result<()> {
    for table in archive.tables() {
        write!(
            output,
            "{}\t",
            table.table_id().as_i32()
        )?;
        write_archive_bytes(output, table.schema())?;
        write!(output, ".")?;
        write_archive_bytes(output, table.name())?;
        write!(output, "\t")?;
        match table.table_data_id() {
            Some(data_id) => write!(output, "{}", data_id.as_i32())?,
            None => write!(output, "-")?,
        }
        writeln!(output)?;
    }
    Ok(())
}

fn write_match<W: Write>(mut output: W, row: &OwnedRow) -> io::Result<()> {
    for (index, field) in row.fields().iter().enumerate() {
        if index != 0 {
            output.write_all(b"\t")?;
        }
        match field {
            OwnedField::Null => output.write_all(b"\\N")?,
            OwnedField::Bytes(bytes) => write_copy_field(&mut output, bytes)?,
        }
    }
    output.write_all(b"\n")
}

fn write_copy_field<W: Write>(output: &mut W, bytes: &[u8]) -> io::Result<()> {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for &byte in bytes {
        match byte {
            b'\\' => output.write_all(b"\\\\")?,
            b'\t' => output.write_all(b"\\t")?,
            b'\n' => output.write_all(b"\\n")?,
            b'\r' => output.write_all(b"\\r")?,
            0x20..=0x7e => output.write_all(&[byte])?,
            _ => output.write_all(&[
                b'\\',
                b'x',
                HEX[usize::from(byte >> 4)],
                HEX[usize::from(byte & 0x0f)],
            ])?,
        }
    }
    Ok(())
}

fn write_optional_archive_string<W: Write>(
    output: &mut W,
    value: Option<&pgdumpx::ArchiveString>,
) -> io::Result<()> {
    match value {
        Some(value) => write_archive_bytes(output, value.as_bytes()),
        None => output.write_all(b"NULL"),
    }
}

fn write_archive_bytes<W: Write>(output: &mut W, bytes: &[u8]) -> io::Result<()> {
    for &byte in bytes {
        match byte {
            b'\\' => output.write_all(b"\\\\")?,
            b'\t' => output.write_all(b"\\t")?,
            b'\n' => output.write_all(b"\\n")?,
            b'\r' => output.write_all(b"\\r")?,
            0x20..=0x7e => output.write_all(&[byte])?,
            _ => write!(output, "\\x{byte:02x}")?,
        }
    }
    Ok(())
}

const fn compression_name(compression: Compression) -> &'static str {
    match compression {
        Compression::None => "none",
        Compression::Gzip => "gzip",
        Compression::Lz4 => "lz4",
        Compression::Zstd => "zstd",
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ErrorKind {
    Usage,
    Runtime,
}

#[derive(Debug)]
struct CliError {
    kind: ErrorKind,
    message: String,
}

impl CliError {
    fn usage(message: impl Into<String>) -> Self {
        Self {
            kind: ErrorKind::Usage,
            message: message.into(),
        }
    }

    fn runtime(message: impl Into<String>) -> Self {
        Self {
            kind: ErrorKind::Runtime,
            message: message.into(),
        }
    }
}

impl fmt::Display for CliError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

#[cfg(test)]
mod tests {
    use super::{render_inspect, render_list};
    use pgdumpx::Archive;
    use std::io::{self, Cursor, Read, Seek, SeekFrom};

    #[test]
    fn metadata_rendering_never_reads_or_seeks_after_archive_open() {
        let reader = MetadataOnlyReader::new(complete_header_with_zero_entries());
        let archive = Archive::open(reader).unwrap();
        archive.get_ref().disallow_io();

        let mut inspect = Vec::new();
        render_inspect(&archive, &mut inspect).unwrap();
        assert_eq!(
            inspect,
            b"archive_version\t1.16.0\ncompression\tnone\t0\ndatabase\tdatabase\nremote_version\t18.4\ndump_version\t18.4\ntoc_entries\t0\ntables\t0\n"
        );

        let mut list = Vec::new();
        render_list(&archive, &mut list).unwrap();
        assert!(list.is_empty());
        assert_eq!(archive.get_ref().io_after_open_attempts(), 0);
    }

    struct MetadataOnlyReader {
        inner: Cursor<Vec<u8>>,
        allow_io: std::cell::Cell<bool>,
        attempts: std::cell::Cell<u64>,
    }

    impl MetadataOnlyReader {
        fn new(bytes: Vec<u8>) -> Self {
            Self {
                inner: Cursor::new(bytes),
                allow_io: std::cell::Cell::new(true),
                attempts: std::cell::Cell::new(0),
            }
        }

        fn disallow_io(&self) {
            self.allow_io.set(false);
        }

        fn io_after_open_attempts(&self) -> u64 {
            self.attempts.get()
        }

        fn record(&self) -> io::Result<()> {
            if self.allow_io.get() {
                Ok(())
            } else {
                self.attempts.set(self.attempts.get() + 1);
                Err(io::Error::other("metadata command attempted payload I/O"))
            }
        }
    }

    impl Read for MetadataOnlyReader {
        fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
            self.record()?;
            self.inner.read(output)
        }
    }

    impl Seek for MetadataOnlyReader {
        fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
            self.record()?;
            self.inner.seek(position)
        }
    }

    fn complete_header_with_zero_entries() -> Vec<u8> {
        let mut bytes = b"PGDMP".to_vec();
        bytes.extend_from_slice(&[1, 16, 0]);
        bytes.push(4);
        bytes.push(8);
        bytes.push(1);
        bytes.push(0);
        for value in [0, 0, 0, 1, 0, 126, 0] {
            write_int(&mut bytes, value);
        }
        write_string(&mut bytes, Some(b"database"));
        write_string(&mut bytes, Some(b"18.4"));
        write_string(&mut bytes, Some(b"18.4"));
        write_int(&mut bytes, 0);
        bytes
    }

    fn write_int(output: &mut Vec<u8>, value: i32) {
        output.push(u8::from(value.is_negative()));
        output.extend_from_slice(&value.unsigned_abs().to_le_bytes());
    }

    fn write_string(output: &mut Vec<u8>, value: Option<&[u8]>) {
        match value {
            Some(bytes) => {
                write_int(output, i32::try_from(bytes.len()).unwrap());
                output.extend_from_slice(bytes);
            }
            None => write_int(output, -1),
        }
    }
}
