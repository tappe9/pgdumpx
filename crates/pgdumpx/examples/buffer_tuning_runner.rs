use pgdumpx::{
    Archive, Compression, EntryReadLimits, ExtractionPlan, TableSelector,
};
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
const PRIMARY_TABLE: &[u8] = b"rows";
const SECONDARY_TABLE: &[u8] = b"rows_secondary";
const DEFAULT_WARMUP: usize = 1;
const DEFAULT_REPETITIONS: usize = 5;
const USAGE: &str = "usage: buffer_tuning_runner <single|multi> <ARCHIVE> \
    [--warmup <N>] [--repetitions <N>]";

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("buffer_tuning_runner: {error}");
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
        "scenario\tcompression\tarchive_version\trepetition\telapsed_ns\tunits\tunit\tunits_per_second\toutcome"
    );
    for repetition in 1..=config.repetitions {
        let result = execute_once(&config)?;
        println!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{:.3}\t{}",
            config.scenario.as_str(),
            result.compression,
            result.archive_version,
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
enum Scenario {
    Single,
    Multi,
}

impl Scenario {
    fn parse(value: &str) -> Result<Self, CliError> {
        match value {
            "single" => Ok(Self::Single),
            "multi" => Ok(Self::Multi),
            _ => Err(CliError::new(format!(
                "unknown scenario {value:?}; expected single or multi"
            ))),
        }
    }

    const fn as_str(self) -> &'static str {
        match self {
            Self::Single => "single",
            Self::Multi => "multi",
        }
    }
}

#[derive(Debug)]
struct Config {
    scenario: Scenario,
    archive: PathBuf,
    warmup: usize,
    repetitions: usize,
}

impl Config {
    fn parse(arguments: Vec<String>) -> Result<Self, CliError> {
        let mut arguments = arguments.into_iter();
        let scenario = arguments
            .next()
            .ok_or_else(|| CliError::new(format!("missing scenario\n{USAGE}")))?;
        let scenario = Scenario::parse(&scenario)?;
        let archive = arguments
            .next()
            .ok_or_else(|| CliError::new(format!("missing archive path\n{USAGE}")))?;

        let mut config = Self {
            scenario,
            archive: PathBuf::from(archive),
            warmup: DEFAULT_WARMUP,
            repetitions: DEFAULT_REPETITIONS,
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
                "-h" | "--help" => return Err(CliError::new(USAGE)),
                _ => {
                    return Err(CliError::new(format!(
                        "unknown argument {argument:?}\n{USAGE}"
                    )));
                }
            }
        }

        Ok(config)
    }
}

fn next_value(
    arguments: &mut impl Iterator<Item = String>,
    flag: &str,
) -> Result<String, CliError> {
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
    let file = File::open(&config.archive)?;
    let mut archive = Archive::open(file)?;
    let compression = compression_name(archive.header().compression());
    let archive_version = archive_version(&archive);

    let (elapsed, units) = match config.scenario {
        Scenario::Single => run_single(&mut archive)?,
        Scenario::Multi => run_multi(&mut archive)?,
    };

    Ok(RunResult {
        compression,
        archive_version,
        elapsed,
        units,
        unit: "decompressed_bytes",
        outcome: "success",
    })
}

fn run_single<R: io::Read + io::Seek>(
    archive: &mut Archive<R>,
) -> Result<(Duration, u64), Box<dyn Error>> {
    let started = Instant::now();
    let table = archive
        .table(SCHEMA, PRIMARY_TABLE)
        .ok_or_else(|| CliError::new("benchmark table bench.rows was not found"))?;
    let data_id = table
        .data_entry_id()
        .ok_or_else(|| CliError::new("benchmark table bench.rows has no TABLE DATA entry"))?;
    let mut output = io::sink();
    let copied = archive.copy_entry_to(data_id, &mut output, EntryReadLimits::unlimited())?;
    Ok((started.elapsed(), copied))
}

fn run_multi<R: io::Read + io::Seek>(
    archive: &mut Archive<R>,
) -> Result<(Duration, u64), Box<dyn Error>> {
    let plan = ExtractionPlan::with_entry_read_limits(
        vec![
            TableSelector::new(SCHEMA, PRIMARY_TABLE),
            TableSelector::new(SCHEMA, SECONDARY_TABLE),
        ],
        EntryReadLimits::unlimited(),
    )?;

    let started = Instant::now();
    let outcomes = plan.execute(archive, |_| Ok::<_, io::Error>(io::sink()))?;
    let elapsed = started.elapsed();
    if outcomes.len() != 2 {
        return Err(CliError::new(format!(
            "expected two completed extraction targets, got {}",
            outcomes.len()
        ))
        .into());
    }
    let copied = outcomes.iter().try_fold(0_u64, |total, outcome| {
        total
            .checked_add(outcome.copied_bytes())
            .ok_or_else(|| CliError::new("multi-table copied-byte counter overflow"))
    })?;
    Ok((elapsed, copied))
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
    fn parses_single_and_multi_scenarios() {
        for (value, expected) in [("single", Scenario::Single), ("multi", Scenario::Multi)] {
            let config = Config::parse(vec![value.into(), "sample.dump".into()]).unwrap();
            assert_eq!(config.scenario, expected);
        }
    }

    #[test]
    fn rejects_zero_repetitions() {
        let error = Config::parse(vec![
            "single".into(),
            "sample.dump".into(),
            "--repetitions".into(),
            "0".into(),
        ])
        .unwrap_err();
        assert!(error.to_string().contains("greater than zero"));
    }
}
