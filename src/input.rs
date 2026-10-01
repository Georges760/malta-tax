//! The JSON file holding the accounts of a self-occupied person, all years
//! together: an asset is deducted over several years, and each year's
//! contributions depend on the income of the year before.

use std::fmt;

use chrono::NaiveDate;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
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
    pub payments: Vec<Payment>,
}

impl Input {
    /// Last year that has income, the one estimated by default.
    pub fn latest_year(&self) -> Option<i32> {
        self.income.iter().map(|entry| entry.month.year).max()
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Taxpayer {
    pub name: String,
    pub tax_status: TaxStatus,
    /// First day of the activity. Class 2 contributions are only due from
    /// the week of that day on.
    pub activity_start: Option<NaiveDate>,
    /// Whether the VAT paid on purchases is claimed back (Article 10
    /// registration). It is then left out of what is deducted.
    #[serde(default)]
    pub vat_recoverable: bool,
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
    /// Amount invoiced. Needed to project a month that is not paid yet.
    pub usd: Option<f64>,
    /// Amount received in EUR; `null` while the month is not paid yet, in
    /// which case it is projected.
    pub eur: Option<f64>,
    /// Day the payment arrived.
    pub received: Option<NaiveDate>,
    pub note: Option<String>,
}

/// An expense deducted in full in the year it is incurred.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Expense {
    pub date: NaiveDate,
    pub description: String,
    /// Price paid, VAT included.
    pub eur: f64,
    /// VAT included in the price.
    #[serde(default)]
    pub vat_eur: f64,
}

/// A capital asset, deducted over several years through wear and tear.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Asset {
    /// Date the asset was bought and put to use.
    pub date: NaiveDate,
    pub description: String,
    pub category: AssetCategory,
    /// Price paid, VAT included.
    pub eur: f64,
    /// VAT included in the price.
    #[serde(default)]
    pub vat_eur: f64,
    #[serde(default = "full_use")]
    pub business_use_percent: f64,
    /// Not bought yet: the date and the amounts are what is expected.
    #[serde(default)]
    pub planned: bool,
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

/// A payment made towards the tax or the contributions of a year.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Payment {
    pub date: NaiveDate,
    /// Year the payment is for, which is not always the year it is made in:
    /// the balance of a year's income tax is paid the year after.
    pub year: i32,
    pub kind: PaymentKind,
    pub eur: f64,
    /// Payment reference number given by the tax administration.
    pub reference: Option<String>,
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
