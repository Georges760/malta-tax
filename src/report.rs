//! Lays an [`Estimate`] out as a [`Report`], and prints a report as plain
//! text. The report is what every interface shows.

use crate::estimate::Estimate;
use crate::input::{Input, PaymentKind};

const LABEL_WIDTH: usize = 46;
const AMOUNT_WIDTH: usize = 12;

#[derive(Debug)]
pub struct Report {
    pub title: String,
    pub sections: Vec<Section>,
}

#[derive(Debug)]
pub struct Section {
    pub title: String,
    pub rows: Vec<Row>,
}

#[derive(Debug)]
pub enum Row {
    /// Column titles of the monthly income table.
    MonthHeader,
    /// One month of income, its cells already formatted.
    Month {
        month: String,
        usd: String,
        eur: String,
        rate: String,
        remark: String,
    },
    Amount {
        label: String,
        value: f64,
        /// Nested under the row above.
        indent: bool,
        /// A total, to stand out where the interface can.
        #[cfg_attr(not(feature = "tui"), allow(dead_code))]
        strong: bool,
        remark: Option<String>,
    },
    Text {
        text: String,
        indent: bool,
    },
    Gap,
}

impl Row {
    fn amount(label: impl Into<String>, value: f64) -> Self {
        Row::Amount {
            label: label.into(),
            value,
            indent: false,
            strong: false,
            remark: None,
        }
    }

    fn total(label: impl Into<String>, value: f64) -> Self {
        Row::Amount {
            label: label.into(),
            value,
            indent: false,
            strong: true,
            remark: None,
        }
    }

    fn nested(label: impl Into<String>, value: f64) -> Self {
        Row::Amount {
            label: label.into(),
            value,
            indent: true,
            strong: false,
            remark: None,
        }
    }

    fn text(text: impl Into<String>) -> Self {
        Row::Text {
            text: text.into(),
            indent: false,
        }
    }

    fn note(text: impl Into<String>) -> Self {
        Row::Text {
            text: text.into(),
            indent: true,
        }
    }
}

pub fn build(input: &Input, estimate: &Estimate) -> Report {
    let year = estimate.year;
    let status = input.taxpayer.tax_status;
    let ssc = &estimate.social_security;

    let mut income = vec![Row::MonthHeader];
    for line in &estimate.months {
        let mut remark = Vec::new();
        match line.received {
            Some(date) => remark.push(format!("received {date}")),
            None if line.projected => remark.push("projected".to_owned()),
            None => {}
        }
        if let Some(note) = &line.note {
            remark.push(format!("({note})"));
        }
        income.push(Row::Month {
            month: line.month.to_string(),
            usd: line.usd.map(money).unwrap_or_default(),
            eur: money(line.eur),
            rate: match line.usd {
                Some(usd) if usd > 0.0 => format!("{:.4}", line.eur / usd),
                _ => String::new(),
            },
            remark: remark.join("  "),
        });
    }
    income.push(Row::Gap);
    let count = |projected| {
        let months = estimate.months.iter();
        months.filter(|m| m.projected == projected).count()
    };
    income.push(Row::amount(
        format!("received ({} months)", count(false)),
        estimate.income_received,
    ));
    if let Some(projection) = &estimate.projection {
        income.push(Row::amount(
            format!("projected ({} months)", count(true)),
            estimate.income_projected,
        ));
        income.push(Row::note(format!(
            "at {:.4} EUR/USD: {}",
            projection.eur_per_usd, projection.source
        )));
    }
    income.push(Row::total("gross income", estimate.gross_income));

    let mut deductions = vec![Row::amount(
        format!("expenses ({})", estimate.expense_count),
        estimate.expenses,
    )];
    for line in &estimate.wear_and_tear {
        deductions.push(Row::amount(
            format!("wear and tear, year {} of {}", line.year_number, line.years),
            line.deduction,
        ));
        deductions.push(Row::note(format!(
            "{}: {} EUR, {} on {}",
            line.description,
            money(line.cost),
            if line.planned { "planned" } else { "bought" },
            line.date
        )));
    }
    deductions.push(Row::total("total deductions", estimate.deductions));
    if estimate.vat_to_reclaim > 0.0 {
        deductions.push(Row::amount(
            "VAT to claim back, not deducted",
            estimate.vat_to_reclaim,
        ));
    }

    let mut income_tax = vec![
        Row::amount("chargeable income", estimate.chargeable_income),
        Row::total(format!("income tax, {status} rates"), estimate.income_tax),
    ];
    if let Some(tax) = estimate.income_tax_without_planned {
        income_tax.push(Row::nested(
            "if the planned purchases slip to next year",
            tax,
        ));
    }
    income_tax.push(Row::text("under the other rates, for comparison:"));
    for (other, tax) in &estimate.income_tax_by_status {
        if *other != status {
            income_tax.push(Row::Amount {
                label: other.to_string(),
                value: *tax,
                indent: true,
                strong: false,
                remark: Some(signed(tax - estimate.income_tax)),
            });
        }
    }

    let social_security = vec![
        Row::text(match ssc.previous_year_net_income {
            Some(income) => format!(
                "based on the {} net income: {} EUR",
                year - 1,
                money(income)
            ),
            None => format!(
                "no {} income: first year of activity, minimum rate",
                year - 1
            ),
        }),
        Row::total(
            format!("{} weeks x {} EUR", ssc.weeks, money(ssc.weekly)),
            ssc.total,
        ),
    ];

    let mut total = vec![
        Row::total("income tax + social security", estimate.total),
        Row::note(format!(
            "{:.1} % of the gross income",
            estimate.set_aside_rate * 100.0
        )),
    ];
    for payment in input.payments.iter().filter(|p| p.year == year) {
        let kind = match payment.kind {
            PaymentKind::ProvisionalTax => "provisional tax",
            PaymentKind::SocialSecurity => "social security",
        };
        total.push(Row::amount(
            format!("paid on {}: {kind}", payment.date),
            -payment.eur,
        ));
        if let Some(reference) = &payment.reference {
            total.push(Row::note(format!("reference {reference}")));
        }
    }
    total.push(Row::total("still to pay", estimate.total - estimate.paid()));
    total.push(Row::nested(
        "income tax",
        estimate.income_tax - estimate.paid_tax,
    ));
    total.push(Row::nested(
        "social security",
        ssc.total - estimate.paid_social_security,
    ));

    let due_so_far = estimate.set_aside_rate * estimate.income_received;
    let set_aside = vec![
        Row::text(format!(
            "{:.1} % of every payment received",
            estimate.set_aside_rate * 100.0
        )),
        Row::amount("on the income received so far", due_so_far),
        Row::total(
            "minus what is already paid",
            (due_so_far - estimate.paid()).max(0.0),
        ),
    ];

    let coming_up = vec![
        Row::text(format!(
            "30 June {}: tax return for {year}, balance of the income tax due",
            year + 1
        )),
        Row::text(format!("30 April, 31 August and 21 December {}:", year + 1)),
        Row::note(format!(
            "Class 2 contributions, based on the {year} net income, in 3 instalments"
        )),
        Row::nested(
            format!("about, at {year} rates"),
            estimate.next_year_social_security,
        ),
        Row::note("provisional tax: 20 %, 30 % and 50 % of the last income tax assessed"),
    ];

    let section = |title: &str, rows| Section {
        title: title.to_owned(),
        rows,
    };
    Report {
        title: format!("Malta tax estimate {year} - {}", input.taxpayer.name),
        sections: vec![
            section("Income", income),
            section("Deductions", deductions),
            section("Income tax", income_tax),
            section("Social security (Class 2)", social_security),
            section(&format!("Total for {year}"), total),
            section("To set aside", set_aside),
            section("Coming up", coming_up),
        ],
    }
}

pub fn print(report: &Report) {
    println!("{}\n{}", report.title, "=".repeat(report.title.len()));
    for section in &report.sections {
        println!("\n{}", section.title);
        for row in &section.rows {
            match row {
                Row::MonthHeader => println!(
                    "  {:<9}{:>12}{:>12}{:>10}",
                    "month", "USD", "EUR", "EUR/USD"
                ),
                Row::Month {
                    month,
                    usd,
                    eur,
                    rate,
                    remark,
                } => {
                    let gap = if remark.is_empty() { "" } else { "  " };
                    println!("  {month:<9}{usd:>12}{eur:>12}{rate:>10}{gap}{remark}");
                }
                Row::Amount {
                    label,
                    value,
                    indent,
                    remark,
                    ..
                } => {
                    let label = format!("{}{label}", if *indent { "  " } else { "" });
                    let remark = remark.as_ref().map(|r| format!("  ({r})"));
                    println!(
                        "  {label:<LABEL_WIDTH$}{:>AMOUNT_WIDTH$}{}",
                        money(*value),
                        remark.unwrap_or_default()
                    );
                }
                Row::Text { text, indent } => {
                    println!("  {}{text}", if *indent { "  " } else { "" });
                }
                Row::Gap => println!(),
            }
        }
    }
}

fn signed(value: f64) -> String {
    let sign = if value > 0.0 { "+" } else { "" };
    format!("{sign}{}", money(value))
}

/// `1234567.891` as `1,234,567.89`.
pub fn money(value: f64) -> String {
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
