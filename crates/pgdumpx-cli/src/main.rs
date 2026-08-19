use pgdumpx::{Archive, FieldRef, OwnedField, OwnedRow};
use std::{
    env,
    ffi::OsString,
    fmt,
    fs::File,
    io::{self, BufReader, Write},
    path::PathBuf,
    process::ExitCode,
};

const NO_MATCH_EXIT: u8 = 1;
const FAILURE_EXIT: u8 = 2;
const USAGE: &str = "usage: pgdumpx find <FILE> <SCHEMA.TABLE> <COLUMN> <VALUE>";

fn main() -> ExitCode {
    let stdout = io::stdout();
    let mut stdout = stdout.lock();

    match run(env::args_os(), &mut stdout) {
        Ok(FindOutcome::Matched) => ExitCode::SUCCESS,
        Ok(FindOutcome::NoMatch) => ExitCode::from(NO_MATCH_EXIT),
        Err(error) => {
            eprintln!("pgdumpx: {error}");
            ExitCode::from(FAILURE_EXIT)
        }
    }
}

fn run<I, W>(arguments: I, stdout: &mut W) -> Result<FindOutcome, CliError>
where
    I: IntoIterator<Item = OsString>,
    W: Write,
{
    let arguments = FindArguments::parse(arguments)?;
    let matched = find(&arguments)?;
    let Some(row) = matched else {
        return Ok(FindOutcome::NoMatch);
    };

    write_row(stdout, &row).map_err(|source| CliError::runtime(format!("stdout error: {source}")))?;
    Ok(FindOutcome::Matched)
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
    let column_index = rows
        .column_index(arguments.column.as_bytes())
        .map_err(|source| CliError::runtime(format!("archive error: {source}")))?
        .ok_or_else(|| {
            CliError::runtime(format!(
                "column {:?} was not found in {}.{}",
                arguments.column, arguments.schema, arguments.table
            ))
        })?;
    let expected = arguments.value.as_bytes();

    rows.find_first(|row| row.field(column_index) == Some(FieldRef::Bytes(expected)))
        .map_err(|source| CliError::runtime(format!("archive error: {source}")))
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
    for &byte in bytes {
        match byte {
            b'\\' => output.write_all(br"\\")?,
            b'\t' => output.write_all(br"\t")?,
            b'\n' => output.write_all(br"\n")?,
            b'\r' => output.write_all(br"\r")?,
            0x20..=0x7e => output.write_all(&[byte])?,
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
    }
    Ok(())
}

#[derive(Debug)]
struct FindArguments {
    file: PathBuf,
    schema: String,
    table: String,
    column: String,
    value: String,
}

impl FindArguments {
    fn parse<I>(arguments: I) -> Result<Self, CliError>
    where
        I: IntoIterator<Item = OsString>,
    {
        let mut arguments = arguments.into_iter();
        let _program = arguments.next();
        let command = required_utf8(arguments.next(), "command")?;
        if command != "find" {
            return Err(CliError::usage("only the find command is supported"));
        }

        let file = arguments
            .next()
            .map(PathBuf::from)
            .ok_or_else(|| CliError::usage("FILE is required"))?;
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
        Ok(Self {
            file,
            schema: schema.to_owned(),
            table: table.to_owned(),
            column,
            value,
        })
    }
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
enum FindOutcome {
    Matched,
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
