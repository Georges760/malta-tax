//! Turns an [`Input`] into the figures of the report.

use chrono::{Datelike, NaiveDate, Weekday};

use crate::input::{Input, Month, PaymentKind, TaxStatus};
use crate::rules::{self, Rules, round_cents};

#[derive(Debug)]
pub struct Estimate {
    pub months: Vec<MonthLine>,
    pub income_received: f64,
    pub income_projected: f64,
    /// Where the rate used for the projected months comes from.
    pub projection: Option<Projection>,
    pub gross_income: f64,
    pub expenses: f64,
    pub wear_and_tear: Vec<WearAndTear>,
    pub deductions: f64,
    pub chargeable_income: f64,
    pub income_tax: f64,
    /// Income tax under every status, for comparison.
    pub income_tax_by_status: Vec<(TaxStatus, f64)>,
    pub social_security: SocialSecurity,
    pub total: f64,
    pub paid_tax: f64,
    pub paid_social_security: f64,
    /// Share of every euro received that goes to tax and contributions.
    pub set_aside_rate: f64,
    /// Class 2 contributions of the following year, based on this year's
    /// net income, at this year's rates.
    pub next_year_social_security: f64,
}

#[derive(Debug)]
pub struct MonthLine {
    pub month: Month,
    pub usd: f64,
    pub eur: f64,
    pub projected: bool,
    pub note: Option<String>,
}

#[derive(Debug)]
pub struct Projection {
    pub eur_per_usd: f64,
    pub source: String,
}

#[derive(Debug)]
pub struct WearAndTear {
    pub description: String,
    pub date: NaiveDate,
    pub cost: f64,
    pub years: i32,
    /// 1 in the year of purchase.
    pub year_number: i32,
    pub deduction: f64,
}

#[derive(Debug)]
pub struct SocialSecurity {
    pub previous_year_net_income: Option<f64>,
    pub weekly: f64,
    pub weeks: u32,
    pub total: f64,
}

impl Estimate {
    pub fn paid(&self) -> f64 {
        self.paid_tax + self.paid_social_security
    }
}

pub fn estimate(input: &Input, rules: &Rules) -> Result<Estimate, String> {
    let year = input.year;

    let (months, projection) = income(input)?;
    let sum = |projected| -> f64 {
        let lines = months.iter().filter(|m| m.projected == projected);
        round_cents(lines.map(|m| m.eur).sum())
    };
    let income_received = sum(false);
    let income_projected = sum(true);
    let gross_income = income_received + income_projected;

    for expense in &input.expenses {
        if expense.date.year() != year {
            return Err(format!(
                "expense {:?} is not dated in {year}",
                expense.description
            ));
        }
        if expense.eur < 0.0 {
            return Err(format!(
                "expense {:?} has a negative amount",
                expense.description
            ));
        }
    }
    let expenses = round_cents(input.expenses.iter().map(|e| e.eur).sum());

    let wear_and_tear = wear_and_tear(input)?;
    let deductions = expenses + wear_and_tear.iter().map(|w| w.deduction).sum::<f64>();
    let chargeable_income = (gross_income - deductions).max(0.0);

    let income_tax = rules.income_tax(input.taxpayer.tax_status, chargeable_income);
    let income_tax_by_status = rules
        .income_tax
        .iter()
        .map(|(status, _)| (*status, rules.income_tax(*status, chargeable_income)))
        .collect();

    let previous_year_net_income = input.social_security.previous_year_net_income_eur;
    let weekly = rules.class2.weekly(previous_year_net_income);
    let weeks = mondays(year, input.taxpayer.activity_start);
    let social_security = SocialSecurity {
        previous_year_net_income,
        weekly,
        weeks,
        total: round_cents(weekly * f64::from(weeks)),
    };

    let paid = |kind| -> f64 {
        let payments = input.payments.iter().filter(|p| p.kind == kind);
        round_cents(payments.map(|p| p.eur).sum())
    };

    let total = income_tax + social_security.total;
    Ok(Estimate {
        months,
        income_received,
        income_projected,
        projection,
        gross_income,
        expenses,
        wear_and_tear,
        deductions,
        chargeable_income,
        income_tax,
        income_tax_by_status,
        total,
        paid_tax: paid(PaymentKind::ProvisionalTax),
        paid_social_security: paid(PaymentKind::SocialSecurity),
        set_aside_rate: if gross_income > 0.0 {
            total / gross_income
        } else {
            0.0
        },
        next_year_social_security: round_cents(
            rules.class2.weekly(Some(chargeable_income)) * f64::from(mondays(year + 1, None)),
        ),
        social_security,
    })
}

/// One line per month, in order. Months without a EUR amount are projected
/// at the rate of the last month received, which already includes the
/// bank's conversion costs, or at the `fx` rate when nothing was received.
fn income(input: &Input) -> Result<(Vec<MonthLine>, Option<Projection>), String> {
    let mut entries: Vec<_> = input.income.iter().collect();
    entries.sort_by_key(|e| e.month);

    for pair in entries.windows(2) {
        if pair[0].month == pair[1].month {
            return Err(format!(
                "month {} appears twice in the income",
                pair[0].month
            ));
        }
    }
    for entry in &entries {
        if entry.month.year != input.year {
            return Err(format!(
                "income month {} is not in {}",
                entry.month, input.year
            ));
        }
        if entry.usd < 0.0 || entry.eur.is_some_and(|eur| eur < 0.0) {
            return Err(format!(
                "income month {} has a negative amount",
                entry.month
            ));
        }
    }

    let last_received = entries
        .iter()
        .rev()
        .find(|e| e.eur.is_some() && e.usd > 0.0);
    let projection = match (last_received, &input.fx) {
        (Some(entry), _) => Some(Projection {
            eur_per_usd: entry.eur.unwrap_or_default() / entry.usd,
            source: format!("rate of {}, the last month received", entry.month),
        }),
        (None, Some(fx)) if fx.usd_per_eur > 0.0 => Some(Projection {
            eur_per_usd: 1.0 / fx.usd_per_eur,
            source: fx
                .source
                .clone()
                .unwrap_or_else(|| "fx.usd_per_eur".to_owned()),
        }),
        (None, Some(_)) => return Err("fx.usd_per_eur must be positive".to_owned()),
        (None, None) => None,
    };

    let mut months = Vec::with_capacity(entries.len());
    for entry in entries {
        let eur = match (entry.eur, &projection) {
            (Some(eur), _) => eur,
            (None, Some(projection)) => round_cents(entry.usd * projection.eur_per_usd),
            (None, None) => {
                return Err(format!(
                    "month {} has no EUR amount and there is no rate to project it: \
                     fill in at least one month or set fx.usd_per_eur",
                    entry.month
                ));
            }
        };
        months.push(MonthLine {
            month: entry.month,
            usd: entry.usd,
            eur,
            projected: entry.eur.is_none(),
            note: entry.note.clone(),
        });
    }

    let used = months.iter().any(|m| m.projected);
    Ok((months, projection.filter(|_| used)))
}

/// Straight-line wear and tear, with a full year's deduction in the year the
/// asset is bought. Assets bought in earlier years stay in the file until
/// they are written off.
fn wear_and_tear(input: &Input) -> Result<Vec<WearAndTear>, String> {
    let mut lines = Vec::new();
    for asset in &input.assets {
        if asset.date.year() > input.year {
            return Err(format!(
                "asset {:?} is dated after {}",
                asset.description, input.year
            ));
        }
        if asset.eur < 0.0 || !(0.0..=100.0).contains(&asset.business_use_percent) {
            return Err(format!(
                "asset {:?} has an invalid amount",
                asset.description
            ));
        }
        let years = rules::wear_and_tear_years(asset.category);
        let year_number = input.year - asset.date.year() + 1;
        if year_number > years {
            continue;
        }
        let cost = round_cents(asset.eur * asset.business_use_percent / 100.0);
        lines.push(WearAndTear {
            description: asset.description.clone(),
            date: asset.date,
            cost,
            years,
            year_number,
            deduction: round_cents(cost / f64::from(years)),
        });
    }
    Ok(lines)
}

/// Number of Mondays in `year`, counted from `from` when it falls in it.
fn mondays(year: i32, from: Option<NaiveDate>) -> u32 {
    let first = NaiveDate::from_ymd_opt(year, 1, 1).expect("valid year");
    let start = from.map_or(first, |from| from.max(first));
    let days = start.iter_days().take_while(|day| day.year() == year);
    days.filter(|day| day.weekday() == Weekday::Mon).count() as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(json: &str) -> Result<Estimate, String> {
        let input: Input = serde_json::from_str(json).expect("valid input");
        estimate(&input, rules::for_year(input.year).expect("known year"))
    }

    #[test]
    fn counts_mondays() {
        assert_eq!(mondays(2026, None), 52);
        assert_eq!(mondays(2024, None), 53);
        let july = NaiveDate::from_ymd_opt(2026, 7, 1);
        assert_eq!(mondays(2026, july), 26);
        assert_eq!(mondays(2026, NaiveDate::from_ymd_opt(2025, 7, 1)), 52);
    }

    #[test]
    fn full_year() {
        let estimate = run(r#"{
            "year": 2026,
            "taxpayer": { "name": "Test", "tax_status": "single" },
            "fx": { "usd_per_eur": 1.25 },
            "income": [
                { "month": "2026-02", "usd": 10000, "eur": null },
                { "month": "2026-01", "usd": 10000, "eur": 10000 },
                { "month": "2026-03", "usd": 10000, "eur": 11700 },
                { "month": "2026-04", "usd": 10000, "eur": null }
            ],
            "expenses": [{ "date": "2026-03-01", "description": "Books", "eur": 100 }],
            "assets": [
                { "date": "2026-09-01", "description": "Laptop", "category": "computer", "eur": 2000 },
                { "date": "2022-05-01", "description": "Old laptop", "category": "computer", "eur": 1000 },
                { "date": "2023-05-01", "description": "Phone", "category": "computer", "eur": 800,
                  "business_use_percent": 50 }
            ],
            "social_security": { "previous_year_net_income_eur": 100000 },
            "payments": [
                { "date": "2026-04-30", "kind": "provisional_tax", "eur": 1000 },
                { "date": "2026-04-30", "kind": "social_security", "eur": 1454.09 }
            ]
        }"#)
        .unwrap();

        // Both open months are projected at March's rate, not at the fx rate.
        assert_eq!(estimate.income_received, 21_700.0);
        assert_eq!(estimate.income_projected, 23_400.0);
        assert!(estimate.months[1].projected && estimate.months[1].eur == 11_700.0);

        // Laptop 2000 / 4, phone 400 / 4, the old laptop is written off.
        assert_eq!(estimate.wear_and_tear.len(), 2);
        assert_eq!(estimate.wear_and_tear[1].year_number, 4);
        assert_eq!(estimate.deductions, 100.0 + 500.0 + 100.0);
        assert_eq!(estimate.chargeable_income, 44_400.0);
        assert_eq!(estimate.income_tax, 44_400.0 * 0.25 - 3_400.0);

        assert_eq!(estimate.social_security.total, round_cents(52.0 * 83.89));
        assert_eq!(
            estimate.total,
            estimate.income_tax + estimate.social_security.total
        );
        assert_eq!(estimate.paid(), 2_454.09);
        // 44 400 × 15 % / 52 = 128.08, capped at the maximum.
        assert_eq!(
            estimate.next_year_social_security,
            round_cents(52.0 * 83.89)
        );
    }

    #[test]
    fn projects_at_fx_rate_when_nothing_is_received() {
        let estimate = run(r#"{
            "year": 2026,
            "taxpayer": { "name": "Test", "tax_status": "single", "activity_start": "2026-07-01" },
            "fx": { "usd_per_eur": 1.25 },
            "income": [{ "month": "2026-07", "usd": 10000, "eur": null }]
        }"#)
        .unwrap();
        assert_eq!(estimate.gross_income, 8_000.0);
        assert_eq!(estimate.income_tax, 0.0);
        assert_eq!(estimate.social_security.total, round_cents(26.0 * 36.18));
    }

    #[test]
    fn rejects_inconsistent_input() {
        let no_rate = run(r#"{
            "year": 2026,
            "taxpayer": { "name": "Test", "tax_status": "single" },
            "income": [{ "month": "2026-07", "usd": 10000, "eur": null }]
        }"#);
        assert!(no_rate.unwrap_err().contains("no rate to project"));

        let wrong_year = run(r#"{
            "year": 2026,
            "taxpayer": { "name": "Test", "tax_status": "single" },
            "income": [{ "month": "2025-12", "usd": 10000, "eur": 11000 }]
        }"#);
        assert!(wrong_year.unwrap_err().contains("is not in 2026"));
    }
}
