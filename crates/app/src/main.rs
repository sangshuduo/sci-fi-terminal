//! sci-fi-terminal entry point: parse flags, then run the native window.

use std::path::PathBuf;
use std::process::ExitCode;

use sci_fi_terminal::config::{ConfigPaths, load_effective};
use sci_fi_terminal::ui::{App, INITIAL_WINDOW, Options};

const USAGE: &str = "\
Usage: sci-fi-terminal [OPTIONS]

Options:
  --config <DIR>    Use DIR as the configuration directory
  --check-config    Validate configuration files and exit
  --safe-mode       Ignore UI overrides, custom themes and saved layout (nothing is deleted)
  -h, --help        Show this help
  -V, --version     Show the version";

enum Command {
    Run(Options),
    CheckConfig(Options),
    Help,
    Version,
}

fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Command, String> {
    let mut options = Options::default();
    let mut check = false;
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--config" => {
                let dir = args.next().ok_or("--config needs a directory")?;
                options.config_dir = Some(PathBuf::from(dir));
            }
            "--check-config" => check = true,
            "--safe-mode" => options.safe_mode = true,
            "-h" | "--help" => return Ok(Command::Help),
            "-V" | "--version" => return Ok(Command::Version),
            other => return Err(format!("unknown argument `{other}`")),
        }
    }
    Ok(if check {
        Command::CheckConfig(options)
    } else {
        Command::Run(options)
    })
}

fn check_config(options: &Options) -> ExitCode {
    let paths = options
        .config_dir
        .clone()
        .map(ConfigPaths::from_dir)
        .or_else(ConfigPaths::platform_default);
    let Some(paths) = paths else {
        eprintln!("no configuration directory is available on this system");
        return ExitCode::FAILURE;
    };
    let loaded = load_effective(&paths, options.safe_mode);
    println!("configuration directory: {}", paths.config_dir.display());
    for source in &loaded.sources {
        println!("loaded: {}", source.display());
    }
    if loaded.diagnostics.is_empty() {
        println!("configuration is valid");
        ExitCode::SUCCESS
    } else {
        for diagnostic in &loaded.diagnostics {
            eprintln!("error: {diagnostic}");
        }
        ExitCode::FAILURE
    }
}

fn main() -> ExitCode {
    let command = match parse_args(std::env::args().skip(1)) {
        Ok(command) => command,
        Err(err) => {
            eprintln!("{err}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    let options = match command {
        Command::Help => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Command::Version => {
            println!("sci-fi-terminal {}", env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
        Command::CheckConfig(options) => return check_config(&options),
        Command::Run(options) => options,
    };
    let result = iced::application(move || App::boot(options.clone()), App::update, App::view)
        .title(App::title)
        .theme(App::theme)
        .subscription(App::subscription)
        .exit_on_close_request(false)
        .antialiasing(true)
        .window_size(INITIAL_WINDOW)
        .run();
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("sci-fi-terminal could not start its window: {err}");
            eprintln!("A GPU with Metal, Vulkan or DirectX 12 support is required.");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Command, String> {
        parse_args(args.iter().map(|s| (*s).to_owned()))
    }

    #[test]
    fn parses_flags() {
        assert!(matches!(parse(&[]), Ok(Command::Run(_))));
        assert!(matches!(
            parse(&["--check-config"]),
            Ok(Command::CheckConfig(_))
        ));
        let Ok(Command::Run(options)) = parse(&["--safe-mode", "--config", "/tmp/x"]) else {
            panic!("expected run");
        };
        assert!(options.safe_mode);
        assert_eq!(options.config_dir, Some(PathBuf::from("/tmp/x")));
        assert!(parse(&["--config"]).is_err());
        assert!(parse(&["--bogus"]).is_err());
        assert!(matches!(parse(&["-V"]), Ok(Command::Version)));
    }
}
