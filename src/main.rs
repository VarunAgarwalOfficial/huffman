//! Command line front end. All logic lives in the library.

use std::fs::{self, File};
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use huffman::container::Stats;
use huffman::Error;

mod completions;

const USAGE: &str = "\
Huffman coding for arbitrary files.

Usage:
  huffman encode [-v] [INPUT [OUTPUT]]   compress
  huffman decode [-v] [INPUT [OUTPUT]]   restore what encode wrote, checking it
  huffman inspect [FILE]                 show the code table, the tree and the
                                         compression summary for a plain file
  huffman completions <bash|zsh|fish>    print a shell completion script
  huffman help [COMMAND]                 show help for a command

Options:
  -v, --verbose   print sizes and ratio to standard error when done
  -h, --help      show this text, or help for the command before it
  -V, --version   show the version

INPUT and OUTPUT default to -, which means standard input and standard
output, so the tool works in a pipe. Files of any size are processed one
megabyte at a time.

Exit status: 0 on success, 1 if a file cannot be read, written or decoded,
2 if the command line is wrong.
";

const HELP_ENCODE: &str = "\
Usage: huffman encode [-v] [INPUT [OUTPUT]]

Compresses INPUT into OUTPUT. The input is cut into blocks of 1 MiB; each
block gets its own canonical Huffman code and a CRC-32, so memory use does
not grow with the file. OUTPUT is written under a temporary name and renamed
when complete, so a failed run leaves no partial file.

  huffman encode notes.txt notes.huf
  tar c photos | huffman encode > photos.tar.huf
";

const HELP_DECODE: &str = "\
Usage: huffman decode [-v] [INPUT [OUTPUT]]

Restores the original bytes. Every block is checked against its CRC-32
before it is written; the end marker is checked against the whole file. On
any damage the exit status is 1, the message names the block, and OUTPUT is
not created. When writing to standard output, the blocks before the damaged
one have already been written.

  huffman decode notes.huf notes.txt
  huffman decode photos.tar.huf - | tar x
";

const HELP_INSPECT: &str = "\
Usage: huffman inspect [FILE]

Reads a plain (not compressed) file and prints each byte value with its
count and code, the code tree, and a summary: container size, entropy and
average code length. With more than 24 distinct bytes the tree is listed by
depth instead of drawn.

  printf 'abracadabra' | huffman inspect
";

const HELP_COMPLETIONS: &str = "\
Usage: huffman completions <bash|zsh|fish>

Prints a completion script for the given shell.

  bash:  source <(huffman completions bash)
  zsh:   source <(huffman completions zsh)      (after compinit)
  fish:  huffman completions fish > ~/.config/fish/completions/huffman.fish
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

fn open_input(path: &str) -> Result<Box<dyn Read>, Failure> {
    if path == "-" {
        Ok(Box::new(io::stdin().lock()))
    } else {
        let file =
            File::open(path).map_err(|e| Failure::runtime(format!("cannot read '{path}': {e}")))?;
        Ok(Box::new(BufReader::new(file)))
    }
}

/// Where the result goes. A regular file is written under a temporary name
/// in the same directory and renamed into place by [`Output::commit`], so a
/// failed run never leaves a file that looks complete.
enum Output {
    Stdout(io::StdoutLock<'static>),
    Direct(BufWriter<File>),
    Staged {
        writer: BufWriter<File>,
        temporary: PathBuf,
        target: PathBuf,
    },
}

impl Output {
    fn create(path: &str) -> io::Result<Self> {
        if path == "-" {
            return Ok(Output::Stdout(io::stdout().lock()));
        }
        let target = Path::new(path);
        // Devices and pipes such as /dev/null are written in place.
        if fs::metadata(target).is_ok_and(|m| !m.is_file()) {
            return Ok(Output::Direct(BufWriter::new(File::create(target)?)));
        }
        let mut name = target.file_name().unwrap_or_default().to_os_string();
        name.push(format!(".part-{}", std::process::id()));
        let temporary = target.with_file_name(name);
        Ok(Output::Staged {
            writer: BufWriter::new(File::create(&temporary)?),
            temporary,
            target: target.to_path_buf(),
        })
    }

    fn writer(&mut self) -> &mut dyn Write {
        match self {
            Output::Stdout(out) => out,
            Output::Direct(out) => out,
            Output::Staged { writer, .. } => writer,
        }
    }

    fn commit(self) -> io::Result<()> {
        match self {
            Output::Stdout(mut out) => out.flush(),
            Output::Direct(mut out) => out.flush(),
            Output::Staged {
                mut writer,
                temporary,
                target,
            } => {
                writer.flush()?;
                drop(writer);
                fs::rename(&temporary, &target)
            }
        }
    }

    fn discard(self) {
        if let Output::Staged {
            writer, temporary, ..
        } = self
        {
            drop(writer);
            let _ = fs::remove_file(temporary);
        }
    }
}

fn write_stdout(text: &str) -> Result<(), Failure> {
    let mut stdout = io::stdout().lock();
    match stdout
        .write_all(text.as_bytes())
        .and_then(|()| stdout.flush())
    {
        Ok(()) => Ok(()),
        // The reader went away (for example `| head`); not an error.
        Err(e) if e.kind() == io::ErrorKind::BrokenPipe => Ok(()),
        Err(e) => Err(Failure::runtime(format!(
            "cannot write standard output: {e}"
        ))),
    }
}

/// Runs encode or decode from `input` to `output`.
fn convert(verb: &str, input: &str, output: &str, verbose: bool) -> Result<(), Failure> {
    let reader = open_input(input)?;
    let mut out = Output::create(output)
        .map_err(|e| Failure::runtime(format!("cannot write '{output}': {e}")))?;
    let result = if verb == "encode" {
        huffman::encode_stream(reader, out.writer())
    } else {
        huffman::decode_stream(reader, out.writer())
    };
    let stats: Stats = match result {
        Ok(stats) => stats,
        // The reader of our standard output went away; not an error.
        Err(Error::Io(io::ErrorKind::BrokenPipe, _)) if output == "-" => return Ok(()),
        Err(e) => {
            out.discard();
            let name = if input == "-" {
                "standard input"
            } else {
                input
            };
            return Err(Failure::runtime(format!("cannot {verb} {name}: {e}")));
        }
    };
    out.commit()
        .map_err(|e| Failure::runtime(format!("cannot write '{output}': {e}")))?;
    if verbose {
        let (from, to) = if verb == "encode" {
            (stats.original_bytes, stats.container_bytes)
        } else {
            (stats.container_bytes, stats.original_bytes)
        };
        let ratio = if stats.original_bytes == 0 {
            "n/a".to_string()
        } else {
            format!(
                "{:.3}",
                stats.container_bytes as f64 / stats.original_bytes as f64
            )
        };
        let _ = writeln!(
            io::stderr(),
            "{from} -> {to} bytes, {} blocks, compressed/original {ratio}",
            stats.blocks
        );
    }
    Ok(())
}

fn help_for(command: &str) -> Result<&'static str, Failure> {
    match command {
        "encode" => Ok(HELP_ENCODE),
        "decode" => Ok(HELP_DECODE),
        "inspect" => Ok(HELP_INSPECT),
        "completions" => Ok(HELP_COMPLETIONS),
        other => Err(Failure::usage(format!("unknown command '{other}'"))),
    }
}

fn run(args: &[String]) -> Result<(), Failure> {
    let mut verbose = false;
    let mut help = false;
    let mut version = false;
    let mut words: Vec<&str> = Vec::new();
    for arg in args {
        match arg.as_str() {
            "-v" | "--verbose" => verbose = true,
            "-h" | "--help" => help = true,
            "-V" | "--version" => version = true,
            "-" => words.push("-"),
            option if option.starts_with('-') => {
                return Err(Failure::usage(format!("unknown option '{option}'")));
            }
            word => words.push(word),
        }
    }
    if version {
        return write_stdout(concat!("huffman ", env!("CARGO_PKG_VERSION"), "\n"));
    }
    if help {
        return write_stdout(match words.first() {
            Some(command) => help_for(command)?,
            None => USAGE,
        });
    }
    match words.as_slice() {
        [verb @ ("encode" | "decode"), files @ ..] if files.len() <= 2 => {
            let input = files.first().copied().unwrap_or("-");
            let output = files.get(1).copied().unwrap_or("-");
            convert(verb, input, output, verbose)
        }
        ["inspect", files @ ..] if files.len() <= 1 => {
            let input = files.first().copied().unwrap_or("-");
            let report = huffman::report::analyze(open_input(input)?)
                .map_err(|e| Failure::runtime(format!("cannot inspect '{input}': {e}")))?;
            write_stdout(&huffman::report::render(&report))
        }
        ["completions", shell] => match completions::script(shell) {
            Some(script) => write_stdout(script),
            None => Err(Failure::usage(format!(
                "unknown shell '{shell}': expected bash, zsh or fish"
            ))),
        },
        ["help"] => write_stdout(USAGE),
        ["help", command] => write_stdout(help_for(command)?),
        ["encode" | "decode", ..] => Err(Failure::usage(format!(
            "'{}' takes at most two file names: [INPUT [OUTPUT]]",
            words[0]
        ))),
        ["inspect", ..] => Err(Failure::usage("'inspect' takes at most one file name")),
        ["completions", ..] => Err(Failure::usage(
            "'completions' takes one argument: bash, zsh or fish",
        )),
        ["help", ..] => Err(Failure::usage("'help' takes at most one command")),
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
