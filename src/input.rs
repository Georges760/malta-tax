//! The JSON file describing one tax year of a self-occupied person.

use std::fmt;

use chrono::NaiveDate;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
    /// Basis year (calendar year in which the income is earned).
    pub year: i32,
    pub taxpayer: Taxpayer,
    /// Only used to project months that have no EUR amount yet, and only
    /// when no month has been received at all.
    pub fx: Option<Fx>,
    pub income: Vec<Income>,
    #[serde(default)]
    pub expenses: Vec<Expense>,
    #[serde(default)]
    pub assets: Vec<Asset>,
    #[serde(default)]
    pub social_security: SocialSecurity,
    #[serde(default)]
    pub payments: Vec<Payment>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Taxpayer {
    pub name: String,
    pub tax_status: TaxStatus,
    /// First day of the activity, when it started during the year. Class 2
    /// contributions are only due for the weeks from that day on.
    pub activity_start: Option<NaiveDate>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaxStatus {
    Single,
    Married,
    MarriedOneChild,
    MarriedTwoOrMoreChildren,
    Parent,
    ParentOneChild,
    ParentTwoOrMoreChildren,
}

impl fmt::Display for TaxStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            TaxStatus::Single => "single",
            TaxStatus::Married => "married",
            TaxStatus::MarriedOneChild => "married, 1 child",
            TaxStatus::MarriedTwoOrMoreChildren => "married, 2+ children",
            TaxStatus::Parent => "parent",
            TaxStatus::ParentOneChild => "parent, 1 child",
            TaxStatus::ParentTwoOrMoreChildren => "parent, 2+ children",
        })
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fx {
    /// USD for one EUR, the way the ECB quotes it.
    pub usd_per_eur: f64,
    pub source: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Income {
    /// Month the invoice relates to, `YYYY-MM`.
    pub month: Month,
    /// Amount invoiced.
    pub usd: f64,
    /// Amount received in EUR; `null` while the month is not paid yet, in
    /// which case it is projected.
    pub eur: Option<f64>,
    pub note: Option<String>,
}

/// An expense deducted in full in the year it is incurred.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Expense {
    pub date: NaiveDate,
    pub description: String,
    /// Excluding VAT when the VAT is recovered, including it otherwise.
    pub eur: f64,
}

/// A capital asset, deducted over several years through wear and tear.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Asset {
    /// Date the asset was bought and put to use.
    pub date: NaiveDate,
    pub description: String,
    pub category: AssetCategory,
    /// Excluding VAT when the VAT is recovered, including it otherwise.
    pub eur: f64,
    #[serde(default = "full_use")]
    pub business_use_percent: f64,
}

fn full_use() -> f64 {
    100.0
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssetCategory {
    Computer,
    Software,
    Furniture,
    OtherMachinery,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SocialSecurity {
    /// Net income of the year before, on which this year's Class 2
    /// contributions are based. `null` when there was none (first year).
    pub previous_year_net_income_eur: Option<f64>,
}

/// A payment already made towards this year's tax or contributions.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Payment {
    pub date: NaiveDate,
    pub kind: PaymentKind,
    pub eur: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PaymentKind {
    ProvisionalTax,
    SocialSecurity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(try_from = "String")]
pub struct Month {
    pub year: i32,
    pub month: u32,
}

impl TryFrom<String> for Month {
    type Error = String;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        let parsed = s.split_once('-').and_then(|(y, m)| {
            let month = Month {
                year: y.parse().ok()?,
                month: m.parse().ok()?,
            };
            (y.len() == 4 && m.len() == 2 && (1..=12).contains(&month.month)).then_some(month)
        });
        parsed.ok_or_else(|| format!("invalid month {s:?}, expected YYYY-MM"))
    }
}

impl fmt::Display for Month {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}", self.year, self.month)
    }
}
