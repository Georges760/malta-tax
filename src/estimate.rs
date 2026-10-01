//! Turns an [`Input`] into the figures of the report for one year.

use chrono::{Datelike, Days, NaiveDate, Weekday};

use crate::input::{Input, Month, PaymentKind, TaxStatus};
use crate::rules::{self, Rules, round_cents};

#[derive(Debug)]
pub struct Estimate {
    /// Basis year (calendar year in which the income is earned).
    pub year: i32,
    pub months: Vec<MonthLine>,
    pub income_received: f64,
    pub income_projected: f64,
    /// Where the rate used for the projected months comes from.
    pub projection: Option<Projection>,
    pub gross_income: f64,
    pub expenses: f64,
    pub expense_count: usize,
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
    /// `None` when the file has no income for the year before.
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

/// Income and deductions of one year, before any tax.
struct Accounts {
    months: Vec<MonthLine>,
    projection: Option<Projection>,
    income_received: f64,
    income_projected: f64,
    expenses: f64,
    expense_count: usize,
    wear_and_tear: Vec<WearAndTear>,
    deductions: f64,
    vat_to_reclaim: f64,
}

impl Accounts {
    fn gross_income(&self) -> f64 {
        self.income_received + self.income_projected
    }

    fn net_income(&self) -> f64 {
        (self.gross_income() - self.deductions).max(0.0)
    }
}

pub fn estimate(input: &Input, year: i32, rules: &Rules) -> Result<Estimate, String> {
    let accounts = accounts(input, year)?;
    let gross_income = accounts.gross_income();
    let chargeable_income = accounts.net_income();

    let status = input.taxpayer.tax_status;
    let income_tax = rules.income_tax(status, chargeable_income);
    let planned = accounts.wear_and_tear.iter().filter(|w| w.planned);
    let planned_deductions: f64 = planned.map(|w| w.deduction).sum();
    let income_tax_without_planned = (planned_deductions > 0.0).then(|| {
        let deductions = accounts.deductions - planned_deductions;
        rules.income_tax(status, (gross_income - deductions).max(0.0))
    });
    let income_tax_by_status = rules
        .income_tax
        .iter()
        .map(|(status, _)| (*status, rules.income_tax(*status, chargeable_income)))
        .collect();

    // The file holds every year since the activity started, so a year
    // without income before this one means there was no activity yet.
    let had_income = |year| input.income.iter().any(|e| e.month.year == year);
    let previous_year_net_income = if had_income(year - 1) {
        Some(self::accounts(input, year - 1)?.net_income())
    } else {
        None
    };
    let weekly = rules.class2.weekly(previous_year_net_income);
    let weeks = contribution_weeks(year, input.taxpayer.activity_start);
    let social_security = SocialSecurity {
        previous_year_net_income,
        weekly,
        weeks,
        total: round_cents(weekly * f64::from(weeks)),
    };

    let paid = |kind| -> f64 {
        let payments = input.payments.iter();
        let payments = payments.filter(|p| p.year == year && p.kind == kind);
        round_cents(payments.map(|p| p.eur).sum())
    };

    let total = income_tax + social_security.total;
    Ok(Estimate {
        year,
        gross_income,
        chargeable_income,
        months: accounts.months,
        income_received: accounts.income_received,
        income_projected: accounts.income_projected,
        projection: accounts.projection,
        expenses: accounts.expenses,
        expense_count: accounts.expense_count,
        wear_and_tear: accounts.wear_and_tear,
        deductions: accounts.deductions,
        vat_to_reclaim: accounts.vat_to_reclaim,
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

fn accounts(input: &Input, year: i32) -> Result<Accounts, String> {
    let (months, projection) = income(input, year)?;
    let sum = |projected| -> f64 {
        let lines = months.iter().filter(|m| m.projected == projected);
        round_cents(lines.map(|m| m.eur).sum())
    };
    let income_received = sum(false);
    let income_projected = sum(true);

    let mut expenses = 0.0;
    let mut expense_count = 0;
    let mut vat_to_reclaim = 0.0;
    for expense in &input.expenses {
        let (cost, vat) = split_vat(input, &expense.description, expense.eur, expense.vat_eur)?;
        if expense.date.year() == year {
            expenses += cost;
            expense_count += 1;
            vat_to_reclaim += vat;
        }
    }
    let expenses = round_cents(expenses);

    let wear_and_tear = wear_and_tear(input, year)?;
    for line in wear_and_tear.iter().filter(|w| w.year_number == 1) {
        vat_to_reclaim += line.vat;
    }

    Ok(Accounts {
        months,
        projection,
        income_received,
        income_projected,
        expenses,
        expense_count,
        deductions: expenses + wear_and_tear.iter().map(|w| w.deduction).sum::<f64>(),
        wear_and_tear,
        vat_to_reclaim: round_cents(vat_to_reclaim),
    })
}

/// One line per month of `year`, in order. Months without a EUR amount are
/// projected at the rate of the last month received up to that year, which
/// already includes the bank's conversion costs, or at the `fx` rate when
/// nothing was received.
fn income(input: &Input, year: i32) -> Result<(Vec<MonthLine>, Option<Projection>), String> {
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
        if entry.usd.is_some_and(|usd| usd < 0.0) || entry.eur.is_some_and(|eur| eur < 0.0) {
            return Err(format!(
                "income month {} has a negative amount",
                entry.month
            ));
        }
    }
    entries.retain(|e| e.month.year <= year);

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

    let mut months = Vec::new();
    for entry in entries.into_iter().filter(|e| e.month.year == year) {
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

/// Straight-line wear and tear of the assets in use in `year`, with a full
/// year's deduction in the year the asset is bought.
fn wear_and_tear(input: &Input, year: i32) -> Result<Vec<WearAndTear>, String> {
    let mut lines = Vec::new();
    for asset in &input.assets {
        if !(0.0..=100.0).contains(&asset.business_use_percent) {
            return Err(format!(
                "asset {:?} has an invalid business use",
                asset.description
            ));
        }
        let (cost, vat) = split_vat(input, &asset.description, asset.eur, asset.vat_eur)?;
        let years = rules::wear_and_tear_years(asset.category);
        let year_number = year - asset.date.year() + 1;
        if !(1..=years).contains(&year_number) {
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

    fn run(json: &str, year: i32) -> Result<Estimate, String> {
        let input: Input = serde_json::from_str(json).expect("valid input");
        estimate(&input, year, rules::for_year(2026).expect("known year"))
    }

    #[test]
    fn counts_contribution_weeks() {
        let date = |y, m, d| NaiveDate::from_ymd_opt(y, m, d);
        assert_eq!(contribution_weeks(2026, None), 52);
        assert_eq!(contribution_weeks(2024, None), 53);
        assert_eq!(contribution_weeks(2026, date(2025, 7, 1)), 52);
        assert_eq!(contribution_weeks(2026, date(2027, 7, 1)), 0);
        // Thursday 11 June 2026: the week of Monday 8 June counts, which gives
        // 13 weeks up to the end of August and 17 for the rest of the year.
        assert_eq!(contribution_weeks(2026, date(2026, 6, 11)), 13 + 17);
        assert_eq!(contribution_weeks(2026, date(2026, 6, 8)), 13 + 17);
        assert_eq!(contribution_weeks(2026, date(2026, 6, 7)), 13 + 17 + 1);
    }

    const SEVERAL_YEARS: &str = r#"{
        "taxpayer": { "name": "Test", "tax_status": "single", "activity_start": "2025-07-01" },
        "fx": { "usd_per_eur": 1.25 },
        "income": [
            { "month": "2025-12", "usd": 26000, "eur": 20800 },
            { "month": "2026-02", "usd": 10000, "eur": null },
            { "month": "2026-01", "usd": 10000, "eur": 10000 },
            { "month": "2026-03", "usd": 10000, "eur": 11700 },
            { "month": "2026-04", "usd": 10000, "eur": null },
            { "month": "2027-01", "usd": 10000, "eur": 13000 }
        ],
        "expenses": [
            { "date": "2025-11-01", "description": "Chair", "eur": 300 },
            { "date": "2026-03-01", "description": "Books", "eur": 100 }
        ],
        "assets": [
            { "date": "2026-09-01", "description": "Laptop", "category": "computer", "eur": 2000 },
            { "date": "2022-05-01", "description": "Old laptop", "category": "computer", "eur": 1000 },
            { "date": "2023-05-01", "description": "Phone", "category": "computer", "eur": 800,
              "business_use_percent": 50 },
            { "date": "2027-02-01", "description": "Desk", "category": "furniture", "eur": 500 }
        ],
        "payments": [
            { "date": "2025-12-21", "year": 2025, "kind": "social_security", "eur": 940.68 },
            { "date": "2026-04-30", "year": 2026, "kind": "provisional_tax", "eur": 1000 },
            { "date": "2026-04-30", "year": 2026, "kind": "social_security", "eur": 1000 },
            { "date": "2027-06-30", "year": 2026, "kind": "provisional_tax", "eur": 500 }
        ]
    }"#;

    #[test]
    fn one_year_out_of_several() {
        let estimate = run(SEVERAL_YEARS, 2026).unwrap();

        // Both open months are projected at March's rate: not at the fx
        // rate, nor at the rate of a later year.
        assert_eq!(estimate.months.len(), 4);
        assert_eq!(estimate.income_received, 21_700.0);
        assert_eq!(estimate.income_projected, 23_400.0);
        assert!(estimate.months[1].projected && estimate.months[1].eur == 11_700.0);

        // Laptop 2000 / 4 and phone 400 / 4. The old laptop is written off
        // and the desk is not bought yet.
        assert_eq!(estimate.expense_count, 1);
        assert_eq!(estimate.wear_and_tear.len(), 2);
        assert_eq!(estimate.wear_and_tear[1].year_number, 4);
        assert_eq!(estimate.deductions, 100.0 + 500.0 + 100.0);
        assert_eq!(estimate.chargeable_income, 44_400.0);
        assert_eq!(estimate.income_tax, 44_400.0 * 0.25 - 3_400.0);

        // Contributions on the 2025 net income: 20 800 - 300 for the chair,
        // 250 for the old laptop and 100 for the phone.
        let ssc = &estimate.social_security;
        assert_eq!(ssc.previous_year_net_income, Some(20_150.0));
        assert_eq!(ssc.weekly, round_cents(20_150.0 * 0.15 / 52.0));
        assert_eq!(ssc.weeks, 52);

        // Payments count for the year they are for, not the year they are
        // made in.
        assert_eq!(estimate.paid_tax, 1_500.0);
        assert_eq!(estimate.paid_social_security, 1_000.0);
        // 44 400 × 15 % / 52 = 128.08, capped at the maximum.
        assert_eq!(
            estimate.next_year_social_security,
            round_cents(52.0 * 83.89)
        );
    }

    #[test]
    fn first_year_has_no_previous_income() {
        let estimate = run(SEVERAL_YEARS, 2025).unwrap();
        assert_eq!(estimate.gross_income, 20_800.0);
        let ssc = &estimate.social_security;
        assert_eq!(ssc.previous_year_net_income, None);
        // 1 July 2025 is a Tuesday: its week and the 26 that follow.
        assert_eq!(ssc.total, round_cents(27.0 * 36.18));
        assert_eq!(estimate.paid_social_security, 940.68);
    }

    #[test]
    fn projects_at_fx_rate_when_nothing_is_received() {
        let estimate = run(
            r#"{
                "taxpayer": { "name": "Test", "tax_status": "single" },
                "fx": { "usd_per_eur": 1.25 },
                "income": [{ "month": "2026-07", "usd": 10000, "eur": null }]
            }"#,
            2026,
        )
        .unwrap();
        assert_eq!(estimate.gross_income, 8_000.0);
        assert_eq!(estimate.income_tax, 0.0);
    }

    #[test]
    fn recoverable_vat_is_not_deducted() {
        let json = |vat_recoverable: bool| {
            format!(
                r#"{{
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

        let recovered = run(&json(true), 2026).unwrap();
        assert_eq!(recovered.deductions, 100.0 + 100.0 + 100.0);
        // The VAT on the desk was claimed back the year it was bought.
        assert_eq!(recovered.vat_to_reclaim, 18.0 + 72.0);

        let not_recovered = run(&json(false), 2026).unwrap();
        assert_eq!(not_recovered.deductions, 118.0 + 118.0 + 118.0);
        assert_eq!(not_recovered.vat_to_reclaim, 0.0);
    }

    #[test]
    fn planned_asset_is_counted_and_flagged() {
        let json = r#"{
            "taxpayer": { "name": "Test", "tax_status": "single" },
            "income": [
                { "month": "2026-05", "usd": 130000, "eur": 100000 },
                { "month": "2027-05", "usd": 130000, "eur": 100000 }
            ],
            "assets": [
                { "date": "2026-08-24", "description": "Screen", "category": "computer", "eur": 400 },
                { "date": "2026-12-15", "description": "Laptop", "category": "computer", "eur": 4000,
                  "planned": true }
            ]
        }"#;
        let estimate = run(json, 2026).unwrap();
        assert_eq!(estimate.deductions, 100.0 + 1_000.0);
        assert_eq!(estimate.income_tax, 98_900.0 * 0.35 - 9_400.0);
        assert_eq!(
            estimate.income_tax_without_planned,
            Some(99_900.0 * 0.35 - 9_400.0)
        );

        // The same assets carry on the year after, as year 2 of 4.
        let estimate = run(json, 2027).unwrap();
        assert_eq!(estimate.deductions, 100.0 + 1_000.0);
        assert!(estimate.wear_and_tear.iter().all(|w| w.year_number == 2));
        assert_eq!(estimate.vat_to_reclaim, 0.0);
    }

    #[test]
    fn received_month_does_not_need_usd() {
        let estimate = run(
            r#"{
                "taxpayer": { "name": "Test", "tax_status": "single" },
                "income": [
                    { "month": "2026-05", "usd": 10000, "eur": 11700 },
                    { "month": "2026-06", "eur": 5000, "received": "2026-07-02" },
                    { "month": "2026-07", "usd": 10000, "eur": null }
                ]
            }"#,
            2026,
        )
        .unwrap();
        // July is projected at May's rate, the last one that is known.
        assert_eq!(estimate.income_received, 16_700.0);
        assert_eq!(estimate.income_projected, 11_700.0);
    }

    #[test]
    fn rejects_inconsistent_input() {
        let no_rate = run(
            r#"{
                "taxpayer": { "name": "Test", "tax_status": "single" },
                "income": [{ "month": "2026-07", "usd": 10000, "eur": null }]
            }"#,
            2026,
        );
        assert!(no_rate.unwrap_err().contains("no rate to project"));

        let twice = run(
            r#"{
                "taxpayer": { "name": "Test", "tax_status": "single" },
                "income": [
                    { "month": "2026-07", "usd": 10000, "eur": 11000 },
                    { "month": "2026-07", "usd": 10000, "eur": 11000 }
                ]
            }"#,
            2026,
        );
        assert!(twice.unwrap_err().contains("appears twice"));
    }
}
