//! Runs the built `huffman` binary end to end.

use std::fs;
use std::io::Write;
use std::process::{Command, Stdio};

const SAMPLE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/samples/lighthouse.txt");

#[test]
fn encode_decode_inspect_and_errors() {
    let dir = std::env::temp_dir().join(format!("huffman-cli-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let path = |name: &str| dir.join(name).to_str().unwrap().to_string();
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_huffman"))
            .args(args)
            .output()
            .unwrap()
    };
    let (packed, restored) = (path("sample.huf"), path("sample.txt"));

    assert!(run(&["encode", SAMPLE, &packed]).status.success());
    assert!(run(&["decode", &packed, &restored]).status.success());
    let original = fs::read(SAMPLE).unwrap();
    assert_eq!(fs::read(&restored).unwrap(), original);
    let packed_len = fs::read(&packed).unwrap().len();
    assert!(packed_len < original.len());

    let inspect = run(&["inspect", SAMPLE]);
    assert!(inspect.status.success());
    let text = String::from_utf8_lossy(&inspect.stdout).into_owned();
    assert!(text.contains("symbol byte      count   share bits  code"));
    assert!(text.contains(&format!("output bytes    {packed_len} (")));
    assert!(text.contains("entropy         4.3104 bits/symbol"));

    // Decoding something that is not a container fails and writes nothing.
    let out = path("out");
    let failed = run(&["decode", SAMPLE, &out]);
    assert_eq!(failed.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&failed.stderr).contains("bad magic"));
    assert!(!dir.join("out").exists());

    // A damaged container is reported with its block and leaves no output.
    let mut bytes = fs::read(&packed).unwrap();
    let middle = bytes.len() / 2;
    bytes[middle] ^= 0x04;
    let damaged = path("damaged.huf");
    fs::write(&damaged, &bytes).unwrap();
    let failed = run(&["decode", &damaged, &out]);
    assert_eq!(failed.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&failed.stderr).contains("block 1"));
    assert_eq!(fs::read_dir(&dir).unwrap().count(), 3);

    assert_eq!(run(&["frobnicate"]).status.code(), Some(2));
    assert!(run(&["--help"]).status.success());
    assert!(run(&["encode", "--help"]).status.success());
    assert!(run(&["--version"]).stdout.starts_with(b"huffman "));
    for shell in ["bash", "zsh", "fish"] {
        assert!(run(&["completions", shell]).status.success());
    }

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn works_as_a_filter_on_pipes() {
    let pipe = |args: &[&str], input: &[u8]| {
        let mut child = Command::new(env!("CARGO_BIN_EXE_huffman"))
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut stdin = child.stdin.take().unwrap();
        let input = input.to_vec();
        let feeder = std::thread::spawn(move || stdin.write_all(&input).unwrap());
        let output = child.wait_with_output().unwrap();
        feeder.join().unwrap();
        assert!(output.status.success());
        output.stdout
    };
    // Larger than one block and larger than a pipe buffer; then the empty input.
    let big: Vec<u8> = fs::read(SAMPLE).unwrap().repeat(1500);
    for original in [big.as_slice(), b""] {
        let packed = pipe(&["encode"], original);
        assert_eq!(pipe(&["decode", "-", "-"], &packed), original);
    }
}
