//! Runs the built `huffman` binary end to end.

use std::fs;
use std::process::Command;

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

    assert_eq!(run(&["frobnicate"]).status.code(), Some(2));
    assert!(run(&["--help"]).status.success());

    fs::remove_dir_all(&dir).unwrap();
}
