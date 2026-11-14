//! Command line front end. All logic lives in the library.

use std::fs;
use std::io::{self, Read, Write};
use std::process::ExitCode;

const USAGE: &str = "\
Huffman coding for arbitrary files.

Usage:
  huffman encode <input> <output>   compress a file
  huffman decode <input> <output>   restore a file written by encode
  huffman inspect <file>            show the code table, the tree and the
                                    compression summary for an uncompressed file
  huffman --help                    show this text

Use - as a file name to read standard input or write standard output.

Exit status: 0 on success, 1 if a file cannot be read, written or decoded,
2 if the command line is wrong.
";

/// A failure with the exit status it maps to.
struct Failure {
    status: u8,
    message: String,
}

impl Failure {
    fn usage(message: impl Into<String>) -> Self {
        Self {
            status: 2,
            message: format!("{}\nRun 'huffman --help' for usage.", message.into()),
        }
    }

    fn runtime(message: impl Into<String>) -> Self {
        Self {
            status: 1,
            message: message.into(),
        }
    }
}

fn read_input(path: &str) -> Result<Vec<u8>, Failure> {
    if path == "-" {
        let mut data = Vec::new();
        io::stdin()
            .lock()
            .read_to_end(&mut data)
            .map_err(|e| Failure::runtime(format!("cannot read standard input: {e}")))?;
        Ok(data)
    } else {
        fs::read(path).map_err(|e| Failure::runtime(format!("cannot read '{path}': {e}")))
    }
}

fn write_stdout(data: &[u8]) -> Result<(), Failure> {
    let mut stdout = io::stdout().lock();
    match stdout.write_all(data).and_then(|()| stdout.flush()) {
        Ok(()) => Ok(()),
        // The reader went away (for example `| head`); not an error.
        Err(e) if e.kind() == io::ErrorKind::BrokenPipe => Ok(()),
        Err(e) => Err(Failure::runtime(format!(
            "cannot write standard output: {e}"
        ))),
    }
}

fn write_output(path: &str, data: &[u8]) -> Result<(), Failure> {
    if path == "-" {
        write_stdout(data)
    } else {
        fs::write(path, data).map_err(|e| Failure::runtime(format!("cannot write '{path}': {e}")))
    }
}

fn run(args: &[String]) -> Result<(), Failure> {
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    if args.is_empty() {
        return Err(Failure::usage("missing command"));
    }
    if args.iter().any(|&a| a == "--help" || a == "-h") {
        return write_stdout(USAGE.as_bytes());
    }
    match args.as_slice() {
        ["encode", input, output] => {
            // The input is read completely before the output is created.
            let data = read_input(input)?;
            let encoded = huffman::encode(&data)
                .map_err(|e| Failure::runtime(format!("cannot encode '{input}': {e}")))?;
            write_output(output, &encoded)
        }
        ["decode", input, output] => {
            let data = read_input(input)?;
            let decoded = huffman::decode(&data)
                .map_err(|e| Failure::runtime(format!("cannot decode '{input}': {e}")))?;
            write_output(output, &decoded)
        }
        ["inspect", input] => {
            let data = read_input(input)?;
            let report = huffman::report::analyze(&data)
                .map_err(|e| Failure::runtime(format!("cannot inspect '{input}': {e}")))?;
            write_stdout(huffman::report::render(&report).as_bytes())
        }
        ["encode" | "decode", ..] => Err(Failure::usage(format!(
            "'{}' takes exactly two arguments: <input> <output>",
            args[0]
        ))),
        ["inspect", ..] => Err(Failure::usage(
            "'inspect' takes exactly one argument: <file>",
        )),
        [other, ..] => Err(Failure::usage(format!("unknown command '{other}'"))),
        [] => Err(Failure::usage("missing command")),
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(failure) => {
            // Ignore a closed stderr; there is nowhere left to report to.
            let _ = writeln!(io::stderr(), "huffman: {}", failure.message);
            ExitCode::from(failure.status)
        }
    }
}
