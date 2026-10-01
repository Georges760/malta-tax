mod estimate;
mod input;
mod report;
mod rules;

use std::error::Error;
use std::process::ExitCode;
use std::{env, fs};

use input::Input;

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let (path, year) = match args.as_slice() {
        [path] => (path, None),
        [path, year] => (path, Some(year.as_str())),
        _ => {
            eprintln!("usage: malta-tax <file.json> [year]");
            return ExitCode::from(2);
        }
    };
    match run(path, year) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(path: &str, year: Option<&str>) -> Result<(), Box<dyn Error>> {
    let json = fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    let input: Input = serde_json::from_str(&json).map_err(|e| format!("{path}: {e}"))?;
    let year = match year {
        Some(year) => year.parse().map_err(|_| format!("invalid year {year:?}"))?,
        None => input
            .latest_year()
            .ok_or_else(|| format!("{path}: no income, so no year to estimate"))?,
    };
    let rules = rules::for_year(year)
        .ok_or_else(|| format!("no tax rules for {year} yet, add them to src/rules.rs"))?;
    let estimate = estimate::estimate(&input, year, rules)?;
    report::print(&input, &estimate);
    Ok(())
}
