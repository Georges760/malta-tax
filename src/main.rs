mod estimate;
mod input;
mod report;
mod rules;
#[cfg(feature = "tui")]
mod tui;

use std::error::Error;
use std::process::ExitCode;
use std::{env, fs};

use estimate::Estimate;
use input::Input;
use report::Report;

const USAGE: &str = "usage: malta-tax [--tui] <file.json> [year]";

fn main() -> ExitCode {
    let (flags, args): (Vec<String>, Vec<String>) =
        env::args().skip(1).partition(|arg| arg.starts_with("--"));
    let interface = match flags.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        [] => Interface::Text,
        ["--tui"] => Interface::Terminal,
        _ => return usage(),
    };
    let (path, year) = match args.as_slice() {
        [path] => (path, None),
        [path, year] => (path, Some(year.as_str())),
        _ => return usage(),
    };
    match run(interface, path, year) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

enum Interface {
    Text,
    Terminal,
}

fn usage() -> ExitCode {
    eprintln!("{USAGE}");
    ExitCode::from(2)
}

fn run(interface: Interface, path: &str, year: Option<&str>) -> Result<(), Box<dyn Error>> {
    let json = fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    let input: Input = serde_json::from_str(&json).map_err(|e| format!("{path}: {e}"))?;
    let year = match year {
        Some(year) => year.parse().map_err(|_| format!("invalid year {year:?}"))?,
        None => input
            .latest_year()
            .ok_or_else(|| format!("{path}: no income, so no year to estimate"))?,
    };
    match interface {
        Interface::Text => {
            let (_, report) = evaluate(&input, year)?;
            report::print(&report);
        }
        #[cfg(feature = "tui")]
        Interface::Terminal => tui::run(&input, year)
            .map_err(|e| format!("the terminal interface needs a terminal: {e}"))?,
        #[cfg(not(feature = "tui"))]
        Interface::Terminal => return Err("built without the `tui` feature".into()),
    }
    Ok(())
}

/// The estimate of a year and the report that presents it.
fn evaluate(input: &Input, year: i32) -> Result<(Estimate, Report), String> {
    let rules = rules::for_year(year)
        .ok_or_else(|| format!("no tax rules for {year} yet, add them to src/rules.rs"))?;
    let estimate = estimate::estimate(input, year, rules)?;
    let report = report::build(input, &estimate);
    Ok((estimate, report))
}
