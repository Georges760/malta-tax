//! Turns an [`Input`] into the figures of the report.

use chrono::{Datelike, Days, NaiveDate, Weekday};

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
    /// VAT on this year's purchases, claimed back rather than deducted.
    pub vat_to_reclaim: f64,
    pub chargeable_income: f64,
    pub income_tax: f64,
    /// Income tax if the planned purchases do not happen this year, when
    /// there are any.
    pub income_tax_without_planned: Option<f64>,
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
    pub usd: Option<f64>,
    pub eur: f64,
    pub projected: bool,
    pub received: Option<NaiveDate>,
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
    /// Business share of the price, without the VAT that is claimed back.
    pub cost: f64,
    /// Business share of the VAT that is claimed back.
    pub vat: f64,
    pub years: i32,
    /// 1 in the year of purchase.
    pub year_number: i32,
    pub deduction: f64,
    /// Not bought yet.
    pub planned: bool,
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

    let mut expenses = 0.0;
    let mut vat_to_reclaim = 0.0;
    for expense in &input.expenses {
        if expense.date.year() != year {
            return Err(format!(
                "expense {:?} is not dated in {year}",
                expense.description
            ));
        }
        let (cost, vat) = split_vat(input, &expense.description, expense.eur, expense.vat_eur)?;
        expenses += cost;
        vat_to_reclaim += vat;
    }
    let expenses = round_cents(expenses);

    let wear_and_tear = wear_and_tear(input)?;
    for line in wear_and_tear.iter().filter(|w| w.year_number == 1) {
        vat_to_reclaim += line.vat;
    }
    let vat_to_reclaim = round_cents(vat_to_reclaim);
    let deductions = expenses + wear_and_tear.iter().map(|w| w.deduction).sum::<f64>();
    let chargeable_income = (gross_income - deductions).max(0.0);

    let status = input.taxpayer.tax_status;
    let income_tax = rules.income_tax(status, chargeable_income);
    let planned = wear_and_tear.iter().filter(|w| w.planned);
    let planned_deductions: f64 = planned.map(|w| w.deduction).sum();
    let income_tax_without_planned = (planned_deductions > 0.0).then(|| {
        let chargeable_income = (gross_income - deductions + planned_deductions).max(0.0);
        rules.income_tax(status, chargeable_income)
    });
    let income_tax_by_status = rules
        .income_tax
        .iter()
        .map(|(status, _)| (*status, rules.income_tax(*status, chargeable_income)))
        .collect();

    let previous_year_net_income = input.social_security.previous_year_net_income_eur;
    let weekly = rules.class2.weekly(previous_year_net_income);
    let weeks = contribution_weeks(year, input.taxpayer.activity_start);
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
        vat_to_reclaim,
        chargeable_income,
        income_tax,
        income_tax_without_planned,
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
            rules.class2.weekly(Some(chargeable_income))
                * f64::from(contribution_weeks(year + 1, None)),
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
        if entry.usd.is_some_and(|usd| usd < 0.0) || entry.eur.is_some_and(|eur| eur < 0.0) {
            return Err(format!(
                "income month {} has a negative amount",
                entry.month
            ));
        }
    }

    let last_rate = entries.iter().rev().find_map(|e| match (e.usd, e.eur) {
        (Some(usd), Some(eur)) if usd > 0.0 => Some((e.month, eur / usd)),
        _ => None,
    });
    let projection = match (last_rate, &input.fx) {
        (Some((month, eur_per_usd)), _) => Some(Projection {
            eur_per_usd,
            source: format!("rate of {month}, the last month received"),
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
        let eur = match (entry.eur, entry.usd, &projection) {
            (Some(eur), _, _) => eur,
            (None, Some(usd), Some(projection)) => round_cents(usd * projection.eur_per_usd),
            (None, Some(_), None) => {
                return Err(format!(
                    "month {} has no EUR amount and there is no rate to project it: \
                     fill in at least one month or set fx.usd_per_eur",
                    entry.month
                ));
            }
            (None, None, _) => {
                return Err(format!(
                    "month {} has neither a USD nor a EUR amount",
                    entry.month
                ));
            }
        };
        months.push(MonthLine {
            month: entry.month,
            usd: entry.usd,
            eur,
            projected: entry.eur.is_none(),
            received: entry.received,
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
        if !(0.0..=100.0).contains(&asset.business_use_percent) {
            return Err(format!(
                "asset {:?} has an invalid business use",
                asset.description
            ));
        }
        let (cost, vat) = split_vat(input, &asset.description, asset.eur, asset.vat_eur)?;
        let years = rules::wear_and_tear_years(asset.category);
        let year_number = input.year - asset.date.year() + 1;
        if year_number > years {
            continue;
        }
        let business_use = asset.business_use_percent / 100.0;
        let cost = round_cents(cost * business_use);
        lines.push(WearAndTear {
            description: asset.description.clone(),
            date: asset.date,
            cost,
            vat: round_cents(vat * business_use),
            years,
            year_number,
            deduction: round_cents(cost / f64::from(years)),
            planned: asset.planned,
        });
    }
    Ok(lines)
}

/// What a purchase costs once the VAT that is claimed back is taken out,
/// and that VAT.
fn split_vat(
    input: &Input,
    description: &str,
    eur: f64,
    vat_eur: f64,
) -> Result<(f64, f64), String> {
    if eur < 0.0 || !(0.0..=eur).contains(&vat_eur) {
        return Err(format!("{description:?} has an invalid amount"));
    }
    Ok(if input.taxpayer.vat_recoverable {
        (eur - vat_eur, vat_eur)
    } else {
        (eur, 0.0)
    })
}

/// Number of contribution weeks in `year`: one per Monday. When the activity
/// starts during the year, the week in which it starts is the first one,
/// even if its Monday is before the start.
fn contribution_weeks(year: i32, from: Option<NaiveDate>) -> u32 {
    let first = NaiveDate::from_ymd_opt(year, 1, 1).expect("valid year");
    let start = from.map_or(first, |from| from.max(first));
    let days = first.iter_days().take_while(|day| day.year() == year);
    let mondays = days.filter(|day| day.weekday() == Weekday::Mon);
    mondays
        .filter(|monday| *monday + Days::new(6) >= start)
        .count() as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(json: &str) -> Result<Estimate, String> {
        let input: Input = serde_json::from_str(json).expect("valid input");
        estimate(&input, rules::for_year(input.year).expect("known year"))
    }

    #[test]
    fn counts_contribution_weeks() {
        let date = |y, m, d| NaiveDate::from_ymd_opt(y, m, d);
        assert_eq!(contribution_weeks(2026, None), 52);
        assert_eq!(contribution_weeks(2024, None), 53);
        assert_eq!(contribution_weeks(2026, date(2025, 7, 1)), 52);
        // Thursday 11 June 2026: the week of Monday 8 June counts, which gives
        // 13 weeks up to the end of August and 17 for the rest of the year.
        assert_eq!(contribution_weeks(2026, date(2026, 6, 11)), 13 + 17);
        assert_eq!(contribution_weeks(2026, date(2026, 6, 8)), 13 + 17);
        assert_eq!(contribution_weeks(2026, date(2026, 6, 7)), 13 + 17 + 1);
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
        // 1 July 2026 is a Wednesday: its week and the 26 that follow.
        assert_eq!(estimate.social_security.total, round_cents(27.0 * 36.18));
    }

    #[test]
    fn recoverable_vat_is_not_deducted() {
        let json = |vat_recoverable: bool| {
            format!(
                r#"{{
                    "year": 2026,
                    "taxpayer": {{ "name": "Test", "tax_status": "single",
                                   "vat_recoverable": {vat_recoverable} }},
                    "income": [{{ "month": "2026-05", "usd": 10000, "eur": 11700 }}],
                    "expenses": [{{ "date": "2026-03-01", "description": "Books", "eur": 118,
                                    "vat_eur": 18 }}],
                    "assets": [
                        {{ "date": "2026-08-24", "description": "Screen", "category": "computer",
                           "eur": 472, "vat_eur": 72 }},
                        {{ "date": "2025-08-24", "description": "Desk", "category": "furniture",
                           "eur": 1180, "vat_eur": 180 }}
                    ]
                }}"#
            )
        };

        let recovered = run(&json(true)).unwrap();
        assert_eq!(recovered.deductions, 100.0 + 100.0 + 100.0);
        // The VAT on the desk was claimed back the year it was bought.
        assert_eq!(recovered.vat_to_reclaim, 18.0 + 72.0);

        let not_recovered = run(&json(false)).unwrap();
        assert_eq!(not_recovered.deductions, 118.0 + 118.0 + 118.0);
        assert_eq!(not_recovered.vat_to_reclaim, 0.0);
    }

    #[test]
    fn planned_asset_is_counted_and_flagged() {
        let estimate = run(r#"{
            "year": 2026,
            "taxpayer": { "name": "Test", "tax_status": "single" },
            "income": [{ "month": "2026-05", "usd": 130000, "eur": 100000 }],
            "assets": [
                { "date": "2026-08-24", "description": "Screen", "category": "computer", "eur": 400 },
                { "date": "2026-12-15", "description": "Laptop", "category": "computer", "eur": 4000,
                  "planned": true }
            ]
        }"#)
        .unwrap();
        assert_eq!(estimate.deductions, 100.0 + 1_000.0);
        assert_eq!(estimate.income_tax, 98_900.0 * 0.35 - 9_400.0);
        assert_eq!(
            estimate.income_tax_without_planned,
            Some(99_900.0 * 0.35 - 9_400.0)
        );
    }

    #[test]
    fn received_month_does_not_need_usd() {
        let estimate = run(r#"{
            "year": 2026,
            "taxpayer": { "name": "Test", "tax_status": "single" },
            "income": [
                { "month": "2026-05", "usd": 10000, "eur": 11700 },
                { "month": "2026-06", "eur": 5000, "received": "2026-07-02" },
                { "month": "2026-07", "usd": 10000, "eur": null }
            ]
        }"#)
        .unwrap();
        // July is projected at May's rate, the last one that is known.
        assert_eq!(estimate.income_received, 16_700.0);
        assert_eq!(estimate.income_projected, 11_700.0);
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
