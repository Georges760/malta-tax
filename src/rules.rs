//! Maltese income tax and social security rules, per basis year.

use crate::input::{AssetCategory, TaxStatus};

/// Income up to `upper` (and above the previous band) is taxed at `rate`.
#[derive(Debug, Clone, Copy)]
pub struct Band {
    pub upper: f64,
    pub rate: f64,
}

/// Class 2 social security contributions of a self-occupied person born on
/// or after 1 January 1962. They are due for every Monday of the year and
/// are based on the net income of the year before.
#[derive(Debug)]
pub struct Class2 {
    pub rate: f64,
    /// Below this income the minimum weekly contribution applies.
    pub min_income: f64,
    pub min_weekly: f64,
    /// From this income on the maximum weekly contribution applies.
    pub max_income: f64,
    pub max_weekly: f64,
}

#[derive(Debug)]
pub struct Rules {
    pub income_tax: &'static [(TaxStatus, [Band; 4])],
    pub class2: Class2,
}

/// The rates are the same for every status, only the first two thresholds
/// move.
const fn bands(tax_free: f64, upper_15: f64) -> [Band; 4] {
    [
        Band {
            upper: tax_free,
            rate: 0.0,
        },
        Band {
            upper: upper_15,
            rate: 0.15,
        },
        Band {
            upper: 60_000.0,
            rate: 0.25,
        },
        Band {
            upper: f64::INFINITY,
            rate: 0.35,
        },
    ]
}

// Income tax: PwC Worldwide Tax Summaries, Malta, basis year 2026
// (https://taxsummaries.pwc.com/malta/individual/taxes-on-personal-income).
// Class 2: MTCA Class 2 social security contribution rates 2026
// (https://mtca.gov.mt/personal-tax/fss/social-security-contribution-rates).
const RULES_2026: Rules = Rules {
    income_tax: &[
        (TaxStatus::Single, bands(12_000.0, 16_000.0)),
        (TaxStatus::Married, bands(15_000.0, 23_000.0)),
        (TaxStatus::MarriedOneChild, bands(17_500.0, 26_500.0)),
        (
            TaxStatus::MarriedTwoOrMoreChildren,
            bands(22_500.0, 32_000.0),
        ),
        (TaxStatus::Parent, bands(13_000.0, 17_500.0)),
        (TaxStatus::ParentOneChild, bands(14_500.0, 21_000.0)),
        (
            TaxStatus::ParentTwoOrMoreChildren,
            bands(18_500.0, 25_500.0),
        ),
    ],
    class2: Class2 {
        rate: 0.15,
        min_income: 12_543.73,
        min_weekly: 36.18,
        max_income: 29_083.36,
        max_weekly: 83.89,
    },
};

pub fn for_year(year: i32) -> Option<&'static Rules> {
    match year {
        2026 => Some(&RULES_2026),
        _ => None,
    }
}

impl Rules {
    pub fn income_tax(&self, status: TaxStatus, chargeable_income: f64) -> f64 {
        let (_, bands) = self
            .income_tax
            .iter()
            .find(|(s, _)| *s == status)
            .expect("every tax status has bands");
        let mut lower = 0.0;
        let mut tax = 0.0;
        for band in bands {
            if chargeable_income > lower {
                tax += (chargeable_income.min(band.upper) - lower) * band.rate;
            }
            lower = band.upper;
        }
        round_cents(tax)
    }
}

impl Class2 {
    /// Weekly contribution, given the net income of the year before (`None`
    /// in the first year of activity, where the minimum applies).
    pub fn weekly(&self, previous_year_net_income: Option<f64>) -> f64 {
        match previous_year_net_income {
            Some(income) if income >= self.max_income => self.max_weekly,
            Some(income) if income >= self.min_income => round_cents(income * self.rate / 52.0),
            _ => self.min_weekly,
        }
    }
}

/// Minimum number of years over which an asset is written off, straight
/// line (Deduction for Wear and Tear of Plant and Machinery Rules,
/// S.L. 123.01).
pub fn wear_and_tear_years(category: AssetCategory) -> i32 {
    match category {
        AssetCategory::Computer | AssetCategory::Software => 4,
        AssetCategory::OtherMachinery => 5,
        AssetCategory::Furniture => 10,
    }
}

pub fn round_cents(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The published tables give, for each band, an amount to subtract from
    /// `income × rate`. Check the top band against them.
    #[test]
    fn top_band_matches_published_subtractions() {
        let subtract = [
            (TaxStatus::Single, 9_400.0),
            (TaxStatus::Married, 10_550.0),
            (TaxStatus::MarriedOneChild, 11_275.0),
            (TaxStatus::MarriedTwoOrMoreChildren, 12_575.0),
            (TaxStatus::Parent, 9_700.0),
            (TaxStatus::ParentOneChild, 10_275.0),
            (TaxStatus::ParentTwoOrMoreChildren, 11_325.0),
        ];
        for (status, subtract) in subtract {
            let tax = RULES_2026.income_tax(status, 140_000.0);
            assert_eq!(tax, 140_000.0 * 0.35 - subtract, "{status}");
        }
    }

    #[test]
    fn lower_bands_match_published_subtractions() {
        let tax = |income| RULES_2026.income_tax(TaxStatus::Single, income);
        assert_eq!(tax(12_000.0), 0.0);
        assert_eq!(tax(14_000.0), 14_000.0 * 0.15 - 1_800.0);
        assert_eq!(tax(40_000.0), 40_000.0 * 0.25 - 3_400.0);
        assert_eq!(tax(60_000.0), 60_000.0 * 0.35 - 9_400.0);

        let tax = |income| RULES_2026.income_tax(TaxStatus::MarriedTwoOrMoreChildren, income);
        assert_eq!(tax(22_500.0), 0.0);
        assert_eq!(tax(30_000.0), 30_000.0 * 0.15 - 3_375.0);
        assert_eq!(tax(50_000.0), 50_000.0 * 0.25 - 6_575.0);
    }

    #[test]
    fn class2_weekly_contribution() {
        let class2 = &RULES_2026.class2;
        assert_eq!(class2.weekly(None), 36.18);
        assert_eq!(class2.weekly(Some(5_000.0)), 36.18);
        assert_eq!(class2.weekly(Some(12_543.73)), 36.18);
        assert_eq!(class2.weekly(Some(20_000.0)), 57.69);
        assert_eq!(class2.weekly(Some(29_083.35)), 83.89);
        assert_eq!(class2.weekly(Some(140_000.0)), 83.89);
    }
}
