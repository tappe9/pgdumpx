use std::{
    env,
    ffi::{OsStr, OsString},
    io::{self, BufWriter, Write},
    process::ExitCode,
};

const FAILURE_EXIT: u8 = 2;
const VERSION: &str = concat!("pgdumpx ", env!("CARGO_PKG_VERSION"));

fn main() -> ExitCode {
    match global_option(env::args_os()) {
        Some(GlobalOption::Help) => write_global_output(application::usage()),
        Some(GlobalOption::Version) => write_global_output(VERSION),
        None => application::launch(),
    }
}

fn global_option<I>(arguments: I) -> Option<GlobalOption>
where
    I: IntoIterator<Item = OsString>,
{
    let mut arguments = arguments.into_iter();
    let _program = arguments.next();
    let option = arguments.next()?;
    if arguments.next().is_some() {
        return None;
    }

    match option.as_os_str() {
        value if value == OsStr::new("--help") || value == OsStr::new("-h") => {
            Some(GlobalOption::Help)
        }
        value if value == OsStr::new("--version") || value == OsStr::new("-V") => {
            Some(GlobalOption::Version)
        }
        _ => None,
    }
}

fn write_global_output(value: &str) -> ExitCode {
    let stdout = io::stdout();
    let mut stdout = BufWriter::new(stdout.lock());
    if let Err(source) = writeln!(stdout, "{value}").and_then(|()| stdout.flush()) {
        eprintln!("pgdumpx: stdout error: {source}");
        return ExitCode::from(FAILURE_EXIT);
    }
    ExitCode::SUCCESS
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum GlobalOption {
    Help,
    Version,
}

mod application {
    include!("main.rs");

    pub(super) fn launch() -> ExitCode {
        main()
    }

    pub(super) fn usage() -> &'static str {
        USAGE
    }
}

#[cfg(test)]
mod tests {
    use super::{GlobalOption, global_option};
    use std::ffi::OsString;

    #[test]
    fn global_options_require_exactly_one_option_argument() {
        assert_eq!(parse(&["--help"]), Some(GlobalOption::Help));
        assert_eq!(parse(&["-h"]), Some(GlobalOption::Help));
        assert_eq!(parse(&["--version"]), Some(GlobalOption::Version));
        assert_eq!(parse(&["-V"]), Some(GlobalOption::Version));
        assert_eq!(parse(&[]), None);
        assert_eq!(parse(&["--help", "unexpected"]), None);
        assert_eq!(parse(&["inspect"]), None);
    }

    fn parse(arguments: &[&str]) -> Option<GlobalOption> {
        global_option(
            ["pgdumpx"]
                .into_iter()
                .chain(arguments.iter().copied())
                .map(OsString::from),
        )
    }
}
