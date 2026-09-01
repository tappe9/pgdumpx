use std::process::{Command, Output};

fn run(arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_pgdumpx"))
        .args(arguments)
        .output()
        .expect("run pgdumpx")
}

#[test]
fn version_reports_the_package_version_on_stdout() {
    let output = run(&["--version"]);

    assert!(output.status.success(), "status: {:?}", output.status.code());
    assert_eq!(output.stdout, b"pgdumpx 0.2.0\n");
    assert!(output.stderr.is_empty());
}

#[test]
fn help_reports_global_usage_on_stdout() {
    let output = run(&["--help"]);

    assert!(output.status.success(), "status: {:?}", output.status.code());
    assert!(output.stderr.is_empty());

    let stdout = String::from_utf8(output.stdout).expect("help is UTF-8");
    assert!(stdout.starts_with("usage:\n"));
    for command in ["inspect", "list", "extract", "find"] {
        assert!(stdout.contains(command), "missing command {command:?}: {stdout}");
    }
    assert!(stdout.ends_with('\n'));
}
