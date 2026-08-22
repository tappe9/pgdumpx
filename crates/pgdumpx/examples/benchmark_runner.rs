use pgdumpx::{Archive, Compression, EntryReadLimits, FieldRef, ScanLimits};
use std::{
    env,
    error::Error,
    fmt,
    fs::File,
    io::{self, Write},
    path::{Path, PathBuf},
    process::ExitCode,
    time::{Duration, Instant},
};

const SCHEMA: &[u8] = b"bench";
const TABLE: &[u8] = b"rows";
const MATCH_COLUMN: &[u8] = b"match_key";
const DEFAULT_WARMUP: usize = 1;
const DEFAULT_REPETITIONS: usize = 5;
const USAGE: &str = "usage: benchmark_runner <open|extract|rows|find> <ARCHIVE> \
    [--warmup <N>] [--repetitions <N>] \
    [--match <early|middle|late|absent>] \
    [--limit-mode <none|raw-bytes|scan-rows|scan-bytes|scan-both>]";

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("benchmark_runner: {error}");
            ExitCode::from(2)
        }
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let arguments = env::args().skip(1).collect::<Vec<_>>();
    if arguments.len() == 1 && matches!(arguments[0].as_str(), "-h" | "--help") {
        println!("{USAGE}");
        return Ok(());
    }

    let config = Config::parse(arguments)?;

    for _ in 0..config.warmup {
        execute_once(&config)?;
    }

    println!(
        "operation\tcompression\tarchive_version\tmatch_position\tlimit_mode\trepetition\telapsed_ns\tunits\tunit\tunits_per_second\toutcome"
    );
    for repetition in 1..=config.repetitions {
        let result = execute_once(&config)?;
        println!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{:.3}\t{}",
            config.operation.as_str(),
            result.compression,
            result.archive_version,
            config.match_position.map_or("-", MatchPosition::as_str),
            result.limit_mode,
            repetition,
            result.elapsed.as_nanos(),
            result.units,
            result.unit,
            result.units_per_second(),
            result.outcome,
        );
    }
    io::stdout().flush()?;
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Operation {
    Open,
    Extract,
    Rows,
    Find,
}

impl Operation {
    fn parse(value: &str) -> Result<Self, CliError> {
        match value {
            "open" => Ok(Self::Open),
            "extract" => Ok(Self::Extract),
            "rows" => Ok(Self::Rows),
            "find" => Ok(Self::Find),
            _ => Err(CliError::new(format!(
                "unknown operation {value:?}; expected open, extract, rows, or find"
            ))),
        }
    }

    const fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Extract => "extract",
            Self::Rows => "rows",
            Self::Find => "find",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LimitMode {
    None,
    RawBytes,
    ScanRows,
    ScanBytes,
    ScanBoth,
}

impl LimitMode {
    fn parse(value: &str) -> Result<Self, CliError> {
        match value {
            "none" => Ok(Self::None),
            "raw-bytes" => Ok(Self::RawBytes),
            "scan-rows" => Ok(Self::ScanRows),
            "scan-bytes" => Ok(Self::ScanBytes),
            "scan-both" => Ok(Self::ScanBoth),
            _ => Err(CliError::new(format!(
                "unknown limit mode {value:?}; expected none, raw-bytes, scan-rows, scan-bytes, or scan-both"
            ))),
        }
    }

    const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::RawBytes => "raw-bytes",
            Self::ScanRows => "scan-rows",
            Self::ScanBytes => "scan-bytes",
            Self::ScanBoth => "scan-both",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MatchPosition {
    Early,
    Middle,
    Late,
    Absent,
}

impl MatchPosition {
    fn parse(value: &str) -> Result<Self, CliError> {
        match value {
            "early" => Ok(Self::Early),
            "middle" => Ok(Self::Middle),
            "late" => Ok(Self::Late),
            "absent" => Ok(Self::Absent),
            _ => Err(CliError::new(format!(
                "unknown match position {value:?}; expected early, middle, late, or absent"
            ))),
        }
    }

    const fn as_str(self) -> &'static str {
        match self {
            Self::Early => "early",
            Self::Middle => "middle",
            Self::Late => "late",
            Self::Absent => "absent",
        }
    }

    const fn target(self) -> &'static [u8] {
        match self {
            Self::Early => b"early",
            Self::Middle => b"middle",
            Self::Late => b"late",
            Self::Absent => b"absent",
        }
    }

    const fn expects_match(self) -> bool {
        !matches!(self, Self::Absent)
    }
}

#[derive(Debug)]
struct Config {
    operation: Operation,
    archive: PathBuf,
    warmup: usize,
    repetitions: usize,
    match_position: Option<MatchPosition>,
    limit_mode: LimitMode,
}

impl Config {
    fn parse(arguments: Vec<String>) -> Result<Self, CliError> {
        let mut arguments = arguments.into_iter();
        let operation = arguments
            .next()
            .ok_or_else(|| CliError::new(format!("missing operation\n{USAGE}")))?;
        let operation = Operation::parse(&operation)?;
        let archive = arguments
            .next()
            .ok_or_else(|| CliError::new(format!("missing archive path\n{USAGE}")))?;

        let mut config = Self {
            operation,
            archive: PathBuf::from(archive),
            warmup: DEFAULT_WARMUP,
            repetitions: DEFAULT_REPETITIONS,
            match_position: None,
            limit_mode: LimitMode::None,
        };

        while let Some(argument) = arguments.next() {
            match argument.as_str() {
                "--warmup" => {
                    let value = next_value(&mut arguments, "--warmup")?;
                    config.warmup = parse_usize(&value, "--warmup")?;
                }
                "--repetitions" => {
                    let value = next_value(&mut arguments, "--repetitions")?;
                    config.repetitions = parse_usize(&value, "--repetitions")?;
                    if config.repetitions == 0 {
                        return Err(CliError::new("--repetitions must be greater than zero"));
                    }
                }
                "--match" => {
                    let value = next_value(&mut arguments, "--match")?;
                    config.match_position = Some(MatchPosition::parse(&value)?);
                }
                "--limit-mode" => {
                    let value = next_value(&mut arguments, "--limit-mode")?;
                    config.limit_mode = LimitMode::parse(&value)?;
                }
                "-h" | "--help" => return Err(CliError::new(USAGE)),
                _ => {
                    return Err(CliError::new(format!(
                        "unknown argument {argument:?}\n{USAGE}"
                    )));
                }
            }
        }

        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> Result<(), CliError> {
        match self.operation {
            Operation::Open | Operation::Rows => {
                if self.match_position.is_some() {
                    return Err(CliError::new(
                        "--match is valid only for the find operation",
                    ));
                }
                if self.limit_mode != LimitMode::None {
                    return Err(CliError::new(
                        "open and rows require --limit-mode none",
                    ));
                }
            }
            Operation::Extract => {
                if self.match_position.is_some() {
                    return Err(CliError::new(
                        "--match is valid only for the find operation",
                    ));
                }
                if !matches!(self.limit_mode, LimitMode::None | LimitMode::RawBytes) {
                    return Err(CliError::new(
                        "extract supports only --limit-mode none or raw-bytes",
                    ));
                }
            }
            Operation::Find => {
                if self.match_position.is_none() {
                    return Err(CliError::new(
                        "find requires --match early, middle, late, or absent",
                    ));
                }
                if self.limit_mode == LimitMode::RawBytes {
                    return Err(CliError::new(
                        "find does not support --limit-mode raw-bytes",
                    ));
                }
            }
        }
        Ok(())
    }
}

fn next_value(arguments: &mut impl Iterator<Item = String>, flag: &str) -> Result<String, CliError> {
    arguments
        .next()
        .ok_or_else(|| CliError::new(format!("{flag} requires a value")))
}

fn parse_usize(value: &str, flag: &str) -> Result<usize, CliError> {
    value
        .parse::<usize>()
        .map_err(|_| CliError::new(format!("{flag} requires a non-negative integer")))
}

struct RunResult {
    compression: &'static str,
    archive_version: String,
    limit_mode: &'static str,
    elapsed: Duration,
    units: u64,
    unit: &'static str,
    outcome: &'static str,
}

impl RunResult {
    fn units_per_second(&self) -> f64 {
        let seconds = self.elapsed.as_secs_f64();
        if seconds == 0.0 {
            0.0
        } else {
            self.units as f64 / seconds
        }
    }
}

fn execute_once(config: &Config) -> Result<RunResult, Box<dyn Error>> {
    match config.operation {
        Operation::Open => run_open(&config.archive),
        Operation::Extract => run_extract(&config.archive, config.limit_mode),
        Operation::Rows => run_rows(&config.archive),
        Operation::Find => {
            let position = config
                .match_position
                .ok_or_else(|| CliError::new("validated find configuration is missing --match"))?;
            run_find(&config.archive, position, config.limit_mode)
        }
    }
}

fn run_open(path: &Path) -> Result<RunResult, Box<dyn Error>> {
    let file = File::open(path)?;
    let started = Instant::now();
    let archive = Archive::open(file)?;
    let elapsed = started.elapsed();
    let entries = u64::try_from(archive.entries().len())
        .map_err(|_| CliError::new("TOC entry count does not fit u64"))?;

    Ok(result_from_archive(
        &archive,
        "structural-default",
        elapsed,
        entries,
        "toc_entries",
        "success",
    ))
}

fn run_extract(path: &Path, limit_mode: LimitMode) -> Result<RunResult, Box<dyn Error>> {
    let file = File::open(path)?;
    let mut archive = Archive::open(file)?;
    let compression = compression_name(archive.header().compression());
    let archive_version = archive_version(&archive);

    let started = Instant::now();
    let table = archive
        .table(SCHEMA, TABLE)
        .ok_or_else(|| CliError::new("benchmark table bench.rows was not found"))?;
    let data_id = table
        .data_entry_id()
        .ok_or_else(|| CliError::new("benchmark table has no TABLE DATA entry"))?;
    let limits = match limit_mode {
        LimitMode::None => EntryReadLimits::unlimited(),
        LimitMode::RawBytes => {
            EntryReadLimits::unlimited().with_max_decompressed_bytes(u64::MAX)
        }
        _ => return Err(CliError::new("invalid extract limit mode after validation").into()),
    };
    let mut output = io::sink();
    let copied = archive.copy_entry_to(data_id, &mut output, limits)?;
    let elapsed = started.elapsed();

    Ok(RunResult {
        compression,
        archive_version,
        limit_mode: limit_mode.as_str(),
        elapsed,
        units: copied,
        unit: "decompressed_bytes",
        outcome: "success",
    })
}

fn run_rows(path: &Path) -> Result<RunResult, Box<dyn Error>> {
    let file = File::open(path)?;
    let mut archive = Archive::open(file)?;
    let compression = compression_name(archive.header().compression());
    let archive_version = archive_version(&archive);

    let started = Instant::now();
    let mut rows = archive.table_rows(SCHEMA, TABLE)?;
    let mut count = 0_u64;
    while rows.next_row()?.is_some() {
        count = count
            .checked_add(1)
            .ok_or_else(|| CliError::new("row counter overflow"))?;
    }
    let elapsed = started.elapsed();

    Ok(RunResult {
        compression,
        archive_version,
        limit_mode: "scan-unlimited",
        elapsed,
        units: count,
        unit: "rows",
        outcome: "success",
    })
}

fn run_find(
    path: &Path,
    position: MatchPosition,
    limit_mode: LimitMode,
) -> Result<RunResult, Box<dyn Error>> {
    let file = File::open(path)?;
    let mut archive = Archive::open(file)?;
    let compression = compression_name(archive.header().compression());
    let archive_version = archive_version(&archive);

    let started = Instant::now();
    let mut rows = archive.table_rows(SCHEMA, TABLE)?;
    let column = rows
        .column_index(MATCH_COLUMN)?
        .ok_or_else(|| CliError::new("benchmark column match_key was not found"))?;
    let target = position.target();
    let mut evaluated = 0_u64;
    let mut predicate = |row: &pgdumpx::Row<'_>| {
        evaluated = evaluated.saturating_add(1);
        matches!(row.field(column), Some(FieldRef::Bytes(bytes)) if bytes == target)
    };

    let matched = match limit_mode {
        LimitMode::None => rows.find_first(&mut predicate)?,
        LimitMode::ScanRows => rows.find_first_with_limits(
            ScanLimits::unlimited().with_max_rows(u64::MAX),
            &mut predicate,
        )?,
        LimitMode::ScanBytes => rows.find_first_with_limits(
            ScanLimits::unlimited().with_max_decompressed_bytes(u64::MAX),
            &mut predicate,
        )?,
        LimitMode::ScanBoth => rows.find_first_with_limits(
            ScanLimits::unlimited()
                .with_max_rows(u64::MAX)
                .with_max_decompressed_bytes(u64::MAX),
            &mut predicate,
        )?,
        LimitMode::RawBytes => {
            return Err(CliError::new("invalid find limit mode after validation").into());
        }
    };
    let elapsed = started.elapsed();

    if matched.is_some() != position.expects_match() {
        return Err(CliError::new(format!(
            "unexpected find result for {}: matched={}",
            position.as_str(),
            matched.is_some()
        ))
        .into());
    }

    Ok(RunResult {
        compression,
        archive_version,
        limit_mode: match limit_mode {
            LimitMode::None => "scan-unlimited",
            _ => limit_mode.as_str(),
        },
        elapsed,
        units: evaluated,
        unit: "evaluated_rows",
        outcome: if matched.is_some() { "matched" } else { "absent" },
    })
}

fn result_from_archive<R>(
    archive: &Archive<R>,
    limit_mode: &'static str,
    elapsed: Duration,
    units: u64,
    unit: &'static str,
    outcome: &'static str,
) -> RunResult {
    RunResult {
        compression: compression_name(archive.header().compression()),
        archive_version: archive_version(archive),
        limit_mode,
        elapsed,
        units,
        unit,
        outcome,
    }
}

fn archive_version<R>(archive: &Archive<R>) -> String {
    let version = archive.header().version();
    format!(
        "{}.{}.{}",
        version.major(),
        version.minor(),
        version.revision()
    )
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

#[derive(Debug)]
struct CliError {
    message: String,
}

impl CliError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for CliError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for CliError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_requires_match_position() {
        let error = Config::parse(vec!["find".into(), "sample.dump".into()]).unwrap_err();
        assert!(error.to_string().contains("find requires --match"));
    }

    #[test]
    fn extract_rejects_scan_limit_modes() {
        let error = Config::parse(vec![
            "extract".into(),
            "sample.dump".into(),
            "--limit-mode".into(),
            "scan-rows".into(),
        ])
        .unwrap_err();
        assert!(error.to_string().contains("extract supports only"));
    }

    #[test]
    fn match_targets_are_distinct() {
        assert_ne!(MatchPosition::Early.target(), MatchPosition::Middle.target());
        assert_ne!(MatchPosition::Middle.target(), MatchPosition::Late.target());
        assert_ne!(MatchPosition::Late.target(), MatchPosition::Absent.target());
    }
}
