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
    let [path] = args.as_slice() else {
        eprintln!("usage: malta-tax <file.json>");
        return ExitCode::from(2);
    };
    match run(path) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(path: &str) -> Result<(), Box<dyn Error>> {
    let json = fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    let input: Input = serde_json::from_str(&json).map_err(|e| format!("{path}: {e}"))?;
    let rules = rules::for_year(input.year).ok_or_else(|| {
        format!(
            "no tax rules for {} yet, add them to src/rules.rs",
            input.year
        )
    })?;
    let estimate = estimate::estimate(&input, rules)?;
    report::print(&input, &estimate);
    Ok(())
}
