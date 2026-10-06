//! uaxfmt: a text formatter with UAX #14 line breaking and Japanese kinsoku.

mod config;
mod console;
mod encoding;
mod format;

use config::{AmbiWidth, Config};
use console::Stream;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Exit code for runtime errors.
const EXIT_ERROR: u8 = 1;
/// Exit code for command line errors.
const EXIT_USAGE: u8 = 2;

#[derive(Default, Debug)]
struct Cli {
    input: Option<String>,
    output: Option<String>,
    config: Option<String>,
    width: Option<usize>,
    hang: Option<usize>,
    ambiwidth: Option<AmbiWidth>,
    print_config: bool,
    help: bool,
    version: bool,
}

fn parse_args(args: Vec<String>) -> Result<Cli, String> {
    let mut cli = Cli::default();
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        let mut chars = arg.chars();
        if chars.next() != Some('-') || arg.len() < 2 {
            return Err(format!("unexpected argument: {arg}"));
        }
        let opt = chars.next().unwrap_or('-');
        let attached: String = chars.collect();
        let flag = |set: &mut bool| {
            if attached.is_empty() {
                *set = true;
                Ok(())
            } else {
                Err(format!("option -{opt} takes no value"))
            }
        };
        match opt {
            'p' => flag(&mut cli.print_config)?,
            'h' => flag(&mut cli.help)?,
            'v' => flag(&mut cli.version)?,
            'i' | 'o' | 'c' | 'w' | 'g' | 'a' => {
                let value = if attached.is_empty() {
                    args.next()
                        .ok_or_else(|| format!("option -{opt} requires a value"))?
                } else {
                    attached
                };
                let invalid = || format!("invalid value for -{opt}: {value}");
                match opt {
                    'i' => cli.input = Some(value),
                    'o' => cli.output = Some(value),
                    'c' => cli.config = Some(value),
                    'w' => cli.width = Some(value.parse().map_err(|_| invalid())?),
                    'g' => cli.hang = Some(value.parse().map_err(|_| invalid())?),
                    _ => cli.ambiwidth = Some(AmbiWidth::parse(&value).ok_or_else(invalid)?),
                }
            }
            _ => return Err(format!("unknown option: -{opt}")),
        }
    }
    Ok(cli)
}

/// Where the settings came from, for the help message.
enum ConfigStatus {
    Loaded,
    NotFound,
    Specified,
}

struct Program {
    /// Base name of the executable, used in messages and for the config file.
    name: String,
    /// Path of the default config file next to the executable.
    default_config: PathBuf,
}

impl Program {
    fn new() -> Program {
        let exe = std::env::current_exe()
            .and_then(std::fs::canonicalize)
            .map(strip_verbatim)
            .unwrap_or_else(|_| PathBuf::from("uaxfmt.exe"));
        let name = exe
            .file_stem()
            .map_or("uaxfmt".to_string(), |s| s.to_string_lossy().into_owned());
        let default_config = exe.with_file_name(format!("{name}.toml"));
        Program {
            name,
            default_config,
        }
    }

    fn error(&self, msg: &str) {
        let _ = console::write_text(Stream::Stderr, &format!("{}: {msg}\n", self.name));
    }

    /// Loads the config file and applies the command line options.
    fn load_config(&self, cli: &Cli) -> Result<(Config, PathBuf, ConfigStatus), String> {
        let mut cfg = Config::default();
        let (path, status) = match &cli.config {
            Some(p) => (PathBuf::from(p), ConfigStatus::Specified),
            None => (self.default_config.clone(), ConfigStatus::Loaded),
        };
        let status = match std::fs::read(&path) {
            Ok(bytes) => {
                let text = decode_config(&bytes).ok_or_else(|| {
                    format!("{}: config file must be UTF-8 or UTF-16LE", path.display())
                })?;
                cfg.apply_toml(&text)
                    .map_err(|e| format!("{}: {e}", path.display()))?;
                status
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound && cli.config.is_none() => {
                ConfigStatus::NotFound
            }
            Err(e) => return Err(format!("{}: {e}", path.display())),
        };
        if let Some(w) = cli.width {
            cfg.width = w;
        }
        if let Some(g) = cli.hang {
            cfg.hang = g;
        }
        if let Some(a) = cli.ambiwidth {
            cfg.ambiwidth = a;
        }
        Ok((cfg, path, status))
    }

    fn help(&self, cfg: &Config, config_line: &str) -> String {
        let n = &self.name;
        format!(
            "\
{n} {VERSION} - text formatter with UAX #14 line breaking and Japanese kinsoku

Usage: {n} [options]
  Reads text, reformats paragraphs, and writes the result.
  Input and output default to stdin and stdout.

Options:
  -i FILE   input file
  -o FILE   output file
  -c FILE   config file
  -w N      line width; 0 joins lines without wrapping        [{w}]
  -g N      max chars allowed to hang past the width; 0 = off [{g}]
  -a N      width of East Asian ambiguous chars: 1, 2 or auto [{a}]
  -p        print the effective config as TOML and exit
  -h        print this help and exit
  -v        print version and exit

  Values in [ ] are the current settings.

Config file:
  {config_line}
  Run '{n} -p' to see all settings.

Examples:
  {n} -i in.txt -o out.txt
  type in.txt | {n} -w 72
  {n} -p -o {example}
",
            w = cfg.width,
            g = cfg.hang,
            a = cfg.ambiwidth.name(),
            example = self.default_config.display(),
        )
    }

    fn run(&self, cli: Cli) -> Result<(), String> {
        let (cfg, _, _) = self.load_config(&cli)?;
        // Validate settings such as list_pattern before doing anything.
        format::Formatter::new(&cfg, false)?;

        if cli.print_config {
            let toml = cfg.to_toml();
            return write_output(cli.output.as_deref(), toml.as_bytes(), Some(&toml));
        }

        let input = cli.input.as_deref().filter(|&p| p != "-");
        let output = cli.output.as_deref().filter(|&p| p != "-");
        if let (Some(i), Some(o)) = (input, output)
            && same_file(Path::new(i), Path::new(o))
        {
            return Err(format!("input and output are the same file: {o}"));
        }

        let bytes = match input {
            Some(p) => std::fs::read(p).map_err(|e| format!("{p}: {e}"))?,
            None => {
                let mut buf = Vec::new();
                std::io::Read::read_to_end(&mut std::io::stdin().lock(), &mut buf)
                    .map_err(|e| format!("cannot read stdin: {e}"))?;
                buf
            }
        };
        let decoded = encoding::decode(&bytes, cfg.encoding)?;

        let cjk = match cfg.ambiwidth {
            AmbiWidth::One => false,
            AmbiWidth::Two => true,
            AmbiWidth::Auto => decoded.encoding.is_legacy_japanese(),
        };
        let formatter = format::Formatter::new(&cfg, cjk)?;

        let text = &decoded.text;
        let newline = match text.find('\n') {
            Some(i) if i > 0 && text.as_bytes()[i - 1] == b'\r' => "\r\n",
            Some(_) => "\n",
            None => "\r\n",
        };
        let trailing_newline = text.ends_with('\n');
        let body = text.strip_suffix('\n').unwrap_or(text);
        let lines: Vec<&str> = if text.is_empty() {
            Vec::new()
        } else {
            body.split('\n')
                .map(|l| l.strip_suffix('\r').unwrap_or(l))
                .collect()
        };

        let mut result = formatter.format(&lines).join(newline);
        if trailing_newline {
            result.push_str(newline);
        }
        let encoded = encoding::encode(&result, decoded.encoding, decoded.bom)?;
        write_output(output, &encoded, Some(&result))
    }
}

/// Writes `bytes` to the file `path`, or to stdout. On a console, `text` is
/// written instead so that it is displayed correctly.
fn write_output(path: Option<&str>, bytes: &[u8], text: Option<&str>) -> Result<(), String> {
    match path.filter(|&p| p != "-") {
        Some(p) => std::fs::write(p, bytes).map_err(|e| format!("{p}: {e}")),
        None => {
            let result = match text {
                Some(t) if console::is_console(Stream::Stdout) => {
                    console::write_text(Stream::Stdout, t)
                }
                _ => console::write_bytes(Stream::Stdout, bytes),
            };
            result.map_err(|e| format!("cannot write stdout: {e}"))
        }
    }
}

/// Decodes a config file: UTF-8 (with or without BOM) or UTF-16LE with BOM.
fn decode_config(bytes: &[u8]) -> Option<String> {
    if let Some(rest) = bytes.strip_prefix(b"\xFF\xFE") {
        return encoding::decode(rest, Some(encoding::Encoding::Utf16Le))
            .ok()
            .map(|d| d.text);
    }
    let rest = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    String::from_utf8(rest.to_vec()).ok()
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// Removes the `\\?\` prefix that `canonicalize` adds on Windows.
fn strip_verbatim(p: PathBuf) -> PathBuf {
    match p.to_str().and_then(|s| s.strip_prefix(r"\\?\")) {
        Some(s) if !s.starts_with("UNC\\") => PathBuf::from(s),
        _ => p,
    }
}

fn main() -> ExitCode {
    let program = Program::new();
    let args: Result<Vec<String>, _> = std::env::args_os()
        .skip(1)
        .map(|a| a.into_string())
        .collect();
    let cli = match args
        .map_err(|a| format!("invalid argument: {}", a.to_string_lossy()))
        .and_then(parse_args)
    {
        Ok(cli) => cli,
        Err(msg) => {
            program.error(&msg);
            let _ = console::write_text(
                Stream::Stderr,
                &format!("Run '{} -h' for help.\n", program.name),
            );
            return ExitCode::from(EXIT_USAGE);
        }
    };

    if cli.version {
        let _ = console::write_text(Stream::Stdout, &format!("{} {VERSION}\n", program.name));
        return ExitCode::SUCCESS;
    }
    if cli.help {
        let (cfg, line) = match program.load_config(&cli) {
            Ok((cfg, path, status)) => {
                let state = match status {
                    ConfigStatus::Loaded => "(loaded)",
                    ConfigStatus::NotFound => "(not found; using defaults)",
                    ConfigStatus::Specified => "(specified with -c)",
                };
                (cfg, format!("{} {state}", path.display()))
            }
            Err(e) => (Config::default(), format!("(error: {e})")),
        };
        let _ = console::write_text(Stream::Stdout, &program.help(&cfg, &line));
        return ExitCode::SUCCESS;
    }

    match program.run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(msg) => {
            program.error(&msg);
            ExitCode::from(EXIT_ERROR)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(s: &str) -> Vec<String> {
        s.split_whitespace().map(String::from).collect()
    }

    #[test]
    fn parses_options() {
        let cli = parse_args(args("-i in.txt -oout.txt -w72 -g 2 -a auto -p")).unwrap();
        assert_eq!(cli.input.as_deref(), Some("in.txt"));
        assert_eq!(cli.output.as_deref(), Some("out.txt"));
        assert_eq!(cli.width, Some(72));
        assert_eq!(cli.hang, Some(2));
        assert_eq!(cli.ambiwidth, Some(AmbiWidth::Auto));
        assert!(cli.print_config);
    }

    #[test]
    fn rejects_bad_options() {
        assert!(
            parse_args(args("-x"))
                .unwrap_err()
                .contains("unknown option")
        );
        assert!(
            parse_args(args("-w"))
                .unwrap_err()
                .contains("requires a value")
        );
        assert!(
            parse_args(args("-w abc"))
                .unwrap_err()
                .contains("invalid value")
        );
        assert!(parse_args(args("-a 3")).is_err());
        assert!(parse_args(args("-px")).is_err());
        assert!(parse_args(args("file.txt")).is_err());
    }
}
