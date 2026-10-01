# malta-tax

Estimates the Maltese income tax and Class 2 social security contributions of
a self-occupied person for one year, from a JSON file, to know how much to set
aside.

    cargo run -- accounts-2026.json

It is an estimate, not a tax return. The rules live in `src/rules.rs`, one set
per year (2026 only for now).

## The JSON file

```json
{
  "year": 2026,
  "taxpayer": {
    "name": "Maria Borg",
    "tax_status": "single",
    "activity_start": "2026-03-01",
    "vat_recoverable": true
  },
  "fx": { "usd_per_eur": 1.1355, "source": "ECB reference rate of 2026-09-30" },
  "income": [
    { "month": "2026-01", "usd": 10000, "eur": 8642.10, "received": "2026-01-30" },
    { "month": "2026-02", "usd": 10000, "eur": null, "note": "not paid yet" }
  ],
  "expenses": [
    { "date": "2026-05-04", "description": "Accountant", "eur": 300 }
  ],
  "assets": [
    { "date": "2026-09-15", "description": "Laptop", "category": "computer", "eur": 2360, "vat_eur": 360 }
  ],
  "social_security": { "previous_year_net_income_eur": 120000 },
  "payments": [
    { "date": "2026-04-30", "kind": "social_security", "eur": 1454.09 }
  ]
}
```

- `tax_status`: `single`, `married`, `married_one_child`,
  `married_two_or_more_children`, `parent`, `parent_one_child` or
  `parent_two_or_more_children`. The report shows the tax under the other
  rates too.
- `activity_start`: optional, only when the activity started during the year.
  Class 2 contributions are counted from the week of that day.
- `income`: one entry per month invoiced. `eur` is what was received; a month
  with `"eur": null` is projected from its `usd` at the rate of the last month
  received, or at `fx.usd_per_eur` when no month was received yet. `usd` and
  `received` are optional on a month that has its `eur`.
- `expenses`: deducted in full. `assets`: deducted over several years
  (`computer` and `software` 4, `other_machinery` 5, `furniture` 10), so an
  asset must be copied to the file of the following years until it is written
  off. `business_use_percent` defaults to 100. `eur` is the price paid, VAT
  included, and `vat_eur` the VAT in it. An asset with `"planned": true` is
  not bought yet: it is counted, and the report also shows the tax without it.
- `vat_recoverable`: `true` when the VAT on purchases is claimed back
  (Article 10 registration); `vat_eur` is then left out of the deductions.
  Defaults to `false`.
- `previous_year_net_income_eur`: this year's Class 2 contributions are based
  on it. `null` in the first year of activity, where the minimum rate applies.
- `payments`: `provisional_tax` and `social_security` already paid for this
  year, with an optional `reference`.
