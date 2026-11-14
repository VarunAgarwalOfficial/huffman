//! Runs the built `huffman` binary end to end.

use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

const BIN: &str = env!("CARGO_BIN_EXE_huffman");
const SAMPLE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/samples/lighthouse.txt");

/// A scratch directory unique to one test, removed on drop.
struct Scratch(PathBuf);

impl Scratch {
    fn new(test: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("huffman-cli-{}-{test}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn path(&self, name: &str) -> String {
        self.0.join(name).to_str().unwrap().to_string()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn run(args: &[&str]) -> Output {
    Command::new(BIN).args(args).output().unwrap()
}

fn run_with_stdin(args: &[&str], input: &[u8]) -> Output {
    let mut child = Command::new(BIN)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input).unwrap();
    child.wait_with_output().unwrap()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn encode_then_decode_restores_the_file() {
    let scratch = Scratch::new("roundtrip");
    let (packed, restored) = (scratch.path("sample.huf"), scratch.path("sample.txt"));

    let output = run(&["encode", SAMPLE, &packed]);
    assert!(output.status.success(), "{}", stderr(&output));
    let output = run(&["decode", &packed, &restored]);
    assert!(output.status.success(), "{}", stderr(&output));

    let original = fs::read(SAMPLE).unwrap();
    assert_eq!(fs::read(&restored).unwrap(), original);
    assert!(fs::read(&packed).unwrap().len() < original.len());
}

#[test]
fn binary_and_empty_files_round_trip() {
    let scratch = Scratch::new("binary");
    let binary: Vec<u8> = (0..5000u32).map(|i| (i * i % 251) as u8).collect();
    for (name, data) in [("binary", binary), ("empty", Vec::new())] {
        let (plain, packed, restored) = (
            scratch.path(name),
            scratch.path(&format!("{name}.huf")),
            scratch.path(&format!("{name}.out")),
        );
        fs::write(&plain, &data).unwrap();
        assert!(run(&["encode", &plain, &packed]).status.success());
        assert!(run(&["decode", &packed, &restored]).status.success());
        assert_eq!(fs::read(&restored).unwrap(), data);
    }
}

#[test]
fn dash_means_stdin_and_stdout() {
    let data = b"streams work too, streams work too";
    let encoded = run_with_stdin(&["encode", "-", "-"], data);
    assert!(encoded.status.success());
    assert_eq!(&encoded.stdout[..4], b"HUFF");
    let decoded = run_with_stdin(&["decode", "-", "-"], &encoded.stdout);
    assert!(decoded.status.success());
    assert_eq!(decoded.stdout, data);
}

#[test]
fn inspect_shows_table_tree_and_summary() {
    let output = run_with_stdin(&["inspect", "-"], b"abracadabra");
    assert!(output.status.success());
    let text = stdout(&output);
    assert!(text.contains("symbol byte      count   share bits  code"));
    assert!(text.contains("'a'    0x61          5  45.45%    1  0"));
    assert!(text.contains("+-0- 'a' 5"));
    assert!(text.contains("input bytes     11"));
    assert!(text.contains("entropy         2.0404 bits/symbol"));
    assert!(text.contains("average code    2.0909 bits/symbol"));
}

#[test]
fn inspect_reports_the_size_encode_produces() {
    let scratch = Scratch::new("inspect-size");
    let packed = scratch.path("sample.huf");
    assert!(run(&["encode", SAMPLE, &packed]).status.success());
    let size = fs::metadata(&packed).unwrap().len();
    let text = stdout(&run(&["inspect", SAMPLE]));
    assert!(
        text.contains(&format!("output bytes    {size} (")),
        "{text}"
    );
}

#[test]
fn help_prints_usage() {
    for flag in ["--help", "-h"] {
        let output = run(&[flag]);
        assert!(output.status.success());
        assert!(stdout(&output).contains("huffman encode <input> <output>"));
    }
}

#[test]
fn usage_errors_exit_with_2() {
    for args in [
        &[][..],
        &["frobnicate"],
        &["encode"],
        &["encode", "only-one"],
        &["decode", "a", "b", "c"],
        &["inspect"],
    ] {
        let output = run(args);
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        assert!(stderr(&output).starts_with("huffman: "), "{args:?}");
        assert!(output.stdout.is_empty());
    }
}

#[test]
fn runtime_errors_exit_with_1_and_write_nothing() {
    let scratch = Scratch::new("errors");
    let out = scratch.path("out");

    let output = run(&["encode", &scratch.path("missing"), &out]);
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).contains("cannot read"));

    // A plain text file is not a container.
    let output = run(&["decode", SAMPLE, &out]);
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).contains("bad magic"));

    // A container cut in half.
    let packed = scratch.path("sample.huf");
    assert!(run(&["encode", SAMPLE, &packed]).status.success());
    let bytes = fs::read(&packed).unwrap();
    let cut = scratch.path("cut.huf");
    fs::write(&cut, &bytes[..bytes.len() / 2]).unwrap();
    let output = run(&["decode", &cut, &out]);
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).contains("truncated"));

    assert!(!std::path::Path::new(&out).exists());
}
