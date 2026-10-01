//! Prints an [`Estimate`] as plain text.

use crate::estimate::Estimate;
use crate::input::{Input, PaymentKind};

const LABEL_WIDTH: usize = 46;
const AMOUNT_WIDTH: usize = 12;

pub fn print(input: &Input, estimate: &Estimate) {
    let year = input.year;
    let status = input.taxpayer.tax_status;

    let title = format!("Malta tax estimate {year} - {}", input.taxpayer.name);
    println!("{title}\n{}", "=".repeat(title.len()));

    section("Income");
    println!(
        "  {:<9}{:>12}{:>12}{:>10}",
        "month", "USD", "EUR", "EUR/USD"
    );
    for line in &estimate.months {
        let rate = if line.usd > 0.0 {
            format!("{:.4}", line.eur / line.usd)
        } else {
            String::new()
        };
        let mut remark = if line.projected {
            "  projected".to_owned()
        } else {
            String::new()
        };
        if let Some(note) = &line.note {
            remark.push_str(&format!("  ({note})"));
        }
        println!(
            "  {:<9}{:>12}{:>12}{rate:>10}{remark}",
            line.month,
            money(line.usd),
            money(line.eur)
        );
    }
    println!();
    let count = |projected| {
        estimate
            .months
            .iter()
            .filter(|m| m.projected == projected)
            .count()
    };
    amount(
        &format!("received ({} months)", count(false)),
        estimate.income_received,
    );
    if let Some(projection) = &estimate.projection {
        amount(
            &format!("projected ({} months)", count(true)),
            estimate.income_projected,
        );
        println!(
            "    at {:.4} EUR/USD: {}",
            projection.eur_per_usd, projection.source
        );
    }
    amount("gross income", estimate.gross_income);

    section("Deductions");
    amount(
        &format!("expenses ({})", input.expenses.len()),
        estimate.expenses,
    );
    for line in &estimate.wear_and_tear {
        amount(
            &format!(
                "wear and tear: {}, year {} of {}",
                line.description, line.year_number, line.years
            ),
            line.deduction,
        );
        println!("    {} EUR bought on {}", money(line.cost), line.date);
    }
    amount("total deductions", estimate.deductions);

    section("Income tax");
    amount("chargeable income", estimate.chargeable_income);
    amount(&format!("income tax, {status} rates"), estimate.income_tax);
    println!("  under the other rates, for comparison:");
    for (other, tax) in &estimate.income_tax_by_status {
        if *other != status {
            println!(
                "    {:<w$}{:>AMOUNT_WIDTH$}  ({})",
                other.to_string(),
                money(*tax),
                signed(tax - estimate.income_tax),
                w = LABEL_WIDTH - 2
            );
        }
    }

    section("Social security (Class 2)");
    let ssc = &estimate.social_security;
    match ssc.previous_year_net_income {
        Some(income) => println!(
            "  based on the {} net income: {} EUR",
            year - 1,
            money(income)
        ),
        None => println!(
            "  no {} net income: first year of activity, minimum rate",
            year - 1
        ),
    }
    amount(
        &format!("{} weeks x {} EUR", ssc.weeks, money(ssc.weekly)),
        ssc.total,
    );

    section(&format!("Total for {year}"));
    amount("income tax + social security", estimate.total);
    println!(
        "    {:.1} % of the gross income",
        estimate.set_aside_rate * 100.0
    );
    for payment in &input.payments {
        let kind = match payment.kind {
            PaymentKind::ProvisionalTax => "provisional tax",
            PaymentKind::SocialSecurity => "social security",
        };
        amount(&format!("paid on {}: {kind}", payment.date), -payment.eur);
    }
    amount("still to pay", estimate.total - estimate.paid());

    section("To set aside");
    println!(
        "  {:.1} % of every payment received",
        estimate.set_aside_rate * 100.0
    );
    let due_so_far = estimate.set_aside_rate * estimate.income_received;
    amount("on the income received so far", due_so_far);
    amount(
        "minus what is already paid",
        (due_so_far - estimate.paid()).max(0.0),
    );

    section("Coming up");
    println!(
        "  30 June {}: tax return for {year}, balance of the income tax due",
        year + 1
    );
    println!("  30 April, 31 August and 21 December {}:", year + 1);
    println!("    Class 2 contributions, based on the {year} net income, in 3 instalments");
    amount(
        &format!("  about, at {year} rates"),
        estimate.next_year_social_security,
    );
    println!("    provisional tax: 20 %, 30 % and 50 % of the last income tax assessed");
}

fn section(title: &str) {
    println!("\n{title}");
}

fn amount(label: &str, value: f64) {
    println!("  {label:<LABEL_WIDTH$}{:>AMOUNT_WIDTH$}", money(value));
}

fn signed(value: f64) -> String {
    let sign = if value > 0.0 { "+" } else { "" };
    format!("{sign}{}", money(value))
}

/// `1234567.891` as `1,234,567.89`.
fn money(value: f64) -> String {
    let cents = (value.abs() * 100.0).round() as u64;
    let digits = (cents / 100).to_string();
    let mut out = String::new();
    if value < 0.0 && cents > 0 {
        out.push('-');
    }
    for (i, digit) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    format!("{out}.{:02}", cents % 100)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_money() {
        assert_eq!(money(0.0), "0.00");
        assert_eq!(money(999.999), "1,000.00");
        assert_eq!(money(1_234_567.891), "1,234,567.89");
        assert_eq!(money(-1_150.0), "-1,150.00");
        assert_eq!(signed(3_175.5), "+3,175.50");
    }
}
