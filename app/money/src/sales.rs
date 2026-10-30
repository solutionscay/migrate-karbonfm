//! The sales money rules on integer cents: line amount (BR-SALES-01), totals and
//! tax (BR-SALES-02), retainer amounts and balance (BR-SALES-06, BR-SALES-07)
//! and the Apply Retainer amount (BR-SALES-18).

use std::fmt;

use crate::qty::{Qty, QTY_SCALE};
use crate::round::div_round_half_away;
use crate::Cents;

/// The item whose lines count as retainer (BR-SALES-07).
pub const RETAINER_ITEM_NAME: &str = "Retainer";

/// A result outside the i64 cents range.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Overflow;

impl fmt::Display for Overflow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("amount out of range")
    }
}

impl std::error::Error for Overflow {}

/// Line amount = quantity x unit price, rounded to the cent half away from
/// zero; an empty quantity counts as 1 (BR-SALES-01). Exact: the product is
/// formed in i128 and divided once.
pub fn line_amount(qty: Option<Qty>, unit_price: Cents) -> Result<Cents, Overflow> {
    let qty = qty.unwrap_or(Qty::ONE);
    let product = i128::from(qty.scaled()) * i128::from(unit_price.0);
    let amount = div_round_half_away(product, i128::from(QTY_SCALE));
    i64::try_from(amount).map(Cents).map_err(|_| Overflow)
}

/// Tax = taxable subtotal x rate in basis points / 10000, rounded to the cent
/// half away from zero (BR-SALES-02; 550 bp = 5.5%).
pub fn tax(taxable_subtotal: Cents, tax_rate_bp: i64) -> Result<Cents, Overflow> {
    let product = i128::from(taxable_subtotal.0) * i128::from(tax_rate_bp);
    let amount = div_round_half_away(product, 10_000);
    i64::try_from(amount).map(Cents).map_err(|_| Overflow)
}

/// The money facts of one sales line, as the command layer loads them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Line {
    pub qty: Option<Qty>,
    pub unit_price: Cents,
    pub taxable: bool,
    /// The line's item is named "Retainer" (BR-SALES-07).
    pub retainer: bool,
}

impl Line {
    pub fn amount(&self) -> Result<Cents, Overflow> {
        line_amount(self.qty, self.unit_price)
    }
}

/// The stored totals of a sales transaction, recomputed from its lines on every
/// save (BR-SALES-02); totals passed in are ignored.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Totals {
    pub subtotal: Cents,
    pub taxable_subtotal: Cents,
    pub total_tax: Cents,
    pub total_amt: Cents,
    pub balance: Cents,
    /// Sum of the retainer lines (BR-SALES-07): `sales_transaction.retainer`.
    pub retainer: Cents,
}

/// Computes the transaction totals from its lines, tax rate, shipping and the
/// sum of the payments applied to it.
pub fn totals(lines: &[Line], tax_rate_bp: i64, ship_amt: Cents, paid: Cents) -> Result<Totals, Overflow> {
    let add = |a: Cents, b: Cents| a.checked_add(b).ok_or(Overflow);
    let mut t = Totals::default();
    for line in lines {
        let amount = line.amount()?;
        t.subtotal = add(t.subtotal, amount)?;
        if line.taxable {
            t.taxable_subtotal = add(t.taxable_subtotal, amount)?;
        }
        if line.retainer {
            t.retainer = add(t.retainer, amount)?;
        }
    }
    t.total_tax = tax(t.taxable_subtotal, tax_rate_bp)?;
    t.total_amt = add(add(t.subtotal, t.total_tax)?, ship_amt)?;
    t.balance = t.total_amt.checked_sub(paid).ok_or(Overflow)?;
    Ok(t)
}

/// Customer retainer balance: the retainer amounts of the customer's posted
/// transactions plus the manual adjustment (BR-SALES-06).
pub fn retainer_balance(
    posted_retainer_amounts: impl IntoIterator<Item = Cents>,
    adjustment: Cents,
) -> Result<Cents, Overflow> {
    posted_retainer_amounts.into_iter().try_fold(adjustment, |acc, a| acc.checked_add(a).ok_or(Overflow))
}

/// Why Apply Retainer is refused (BR-SALES-18). A locked invoice is refused by
/// the command before it gets here (AP-05).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApplyRetainerRefused {
    NothingDue,
    NoRetainerAvailable,
}

/// The available retainer: the balance less the retainer credit lines already on
/// the customer's unposted transactions (those lines are negative), so one
/// deposit cannot be applied twice (BR-SALES-18).
pub fn available_retainer(
    retainer_balance: Cents,
    unposted_retainer_lines: impl IntoIterator<Item = Cents>,
) -> Result<Cents, Overflow> {
    // The same sum as the balance itself, starting from the balance.
    self::retainer_balance(unposted_retainer_lines, retainer_balance)
}

/// The amount of the retainer credit line: -min(balance due, available retainer).
pub fn apply_retainer_amount(balance_due: Cents, available: Cents) -> Result<Cents, ApplyRetainerRefused> {
    if balance_due <= Cents::ZERO {
        return Err(ApplyRetainerRefused::NothingDue);
    }
    if available <= Cents::ZERO {
        return Err(ApplyRetainerRefused::NoRetainerAvailable);
    }
    Ok(-balance_due.min(available))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn qty(s: &str) -> Option<Qty> {
        Some(Qty::parse(s).unwrap())
    }

    fn line(q: &str, unit: i64, taxable: bool) -> Line {
        Line { qty: qty(q), unit_price: Cents(unit), taxable, retainer: false }
    }

    // The examples of BR-SALES-01 in 07.
    #[test]
    fn br_sales_01_examples() {
        assert_eq!(line_amount(qty("2"), Cents(120_000)), Ok(Cents(240_000)));
        assert_eq!(line_amount(qty("6"), Cents(9_500)), Ok(Cents(57_000)));
        assert_eq!(line_amount(qty("1.5"), Cents(3_333)), Ok(Cents(5_000))); // 4999.5 rounds up
        assert_eq!(line_amount(None, Cents(45_000)), Ok(Cents(45_000)));
    }

    #[test]
    fn half_cent_lines_round_away_from_zero() {
        assert_eq!(line_amount(qty("0.5"), Cents(1)), Ok(Cents(1))); // 0.5
        assert_eq!(line_amount(qty("0.5"), Cents(3)), Ok(Cents(2))); // 1.5
        assert_eq!(line_amount(qty("0.5"), Cents(5)), Ok(Cents(3))); // 2.5, not 2
        assert_eq!(line_amount(qty("0.5"), Cents(-5)), Ok(Cents(-3))); // retainer-style credit
        assert_eq!(line_amount(qty("-0.5"), Cents(5)), Ok(Cents(-3)));
        assert_eq!(line_amount(qty("0.25"), Cents(2)), Ok(Cents(1))); // 0.5
        assert_eq!(line_amount(qty("0.0001"), Cents(5_000)), Ok(Cents(1))); // 0.5
        assert_eq!(line_amount(qty("0.0001"), Cents(4_999)), Ok(Cents(0))); // 0.4999
    }

    #[test]
    fn decimal_quantities() {
        assert_eq!(line_amount(qty("1.25"), Cents(9_500)), Ok(Cents(11_875)));
        assert_eq!(line_amount(qty("0.3333"), Cents(9_500)), Ok(Cents(3_166))); // 3166.35
        assert_eq!(line_amount(qty("7.75"), Cents(3_250)), Ok(Cents(25_188))); // 25187.5
        assert_eq!(line_amount(qty("0"), Cents(9_500)), Ok(Cents(0)));
    }

    #[test]
    fn line_amount_overflow() {
        assert_eq!(line_amount(qty("2"), Cents(i64::MAX)), Err(Overflow));
        assert_eq!(line_amount(qty("1"), Cents(i64::MAX)), Ok(Cents(i64::MAX)));
    }

    #[test]
    fn tax_examples() {
        // INV-1048: taxable 13000 x 550 bp = 715.
        assert_eq!(tax(Cents(13_000), 550), Ok(Cents(715)));
        assert_eq!(tax(Cents(100), 550), Ok(Cents(6))); // 5.5 -> 6
        assert_eq!(tax(Cents(300), 550), Ok(Cents(17))); // 16.5 -> 17, not 16
        assert_eq!(tax(Cents(-300), 550), Ok(Cents(-17)));
        assert_eq!(tax(Cents(1_999), 625), Ok(Cents(125))); // 124.9375
        assert_eq!(tax(Cents(13_000), 0), Ok(Cents(0)));
        assert_eq!(tax(Cents(i64::MAX), 20_000), Err(Overflow));
    }

    #[test]
    fn inv_1048_totals() {
        let lines = [
            line("2", 120_000, false),
            line("1", 45_000, false),
            line("6", 9_500, false),
            line("4", 3_250, true),
            Line { qty: qty("1"), unit_price: Cents(-50_000), taxable: false, retainer: true },
        ];
        let t = totals(&lines, 550, Cents::ZERO, Cents(100_000)).unwrap();
        assert_eq!(
            t,
            Totals {
                subtotal: Cents(305_000),
                taxable_subtotal: Cents(13_000),
                total_tax: Cents(715),
                total_amt: Cents(305_715),
                balance: Cents(205_715),
                retainer: Cents(-50_000),
            }
        );
        assert_eq!(t.balance.to_string(), "$2,057.15");
    }

    #[test]
    fn tax_is_rounded_once_on_the_subtotal_not_per_line() {
        // Three taxable lines of 100 at 5.5%: per-line tax would be 3 x 6 = 18.
        let lines = [line("1", 100, true), line("1", 100, true), line("1", 100, true)];
        assert_eq!(totals(&lines, 550, Cents::ZERO, Cents::ZERO).unwrap().total_tax, Cents(17));
    }

    #[test]
    fn shipping_and_overpayment() {
        let t = totals(&[line("1", 1_000, true)], 550, Cents(250), Cents(2_000)).unwrap();
        assert_eq!(t.total_amt, Cents(1_305));
        assert_eq!(t.balance, Cents(-695));
    }

    #[test]
    fn retainer_balance_counts_posted_amounts_and_adjustment() {
        // Halden & Pryce: RET-0042 3000.00, INV-1035 -1000.00, INV-1048 -500.00.
        let posted = [Cents(300_000), Cents(-100_000), Cents(-50_000)];
        assert_eq!(retainer_balance(posted, Cents::ZERO), Ok(Cents(150_000)));
        assert_eq!(retainer_balance(posted, Cents(-10_000)), Ok(Cents(140_000)));
        assert_eq!(retainer_balance([], Cents::ZERO), Ok(Cents::ZERO));
    }

    // The examples of BR-SALES-18 in 07.
    #[test]
    fn br_sales_18_examples() {
        assert_eq!(apply_retainer_amount(Cents(255_715), Cents(200_000)), Ok(Cents(-200_000)));
        assert_eq!(apply_retainer_amount(Cents(30_000), Cents(200_000)), Ok(Cents(-30_000)));
        assert_eq!(apply_retainer_amount(Cents(30_000), Cents::ZERO), Err(ApplyRetainerRefused::NoRetainerAvailable));
        assert_eq!(apply_retainer_amount(Cents::ZERO, Cents(200_000)), Err(ApplyRetainerRefused::NothingDue));
    }

    #[test]
    fn two_drafts_cannot_use_the_same_deposit() {
        let balance = Cents(200_000);
        let first = apply_retainer_amount(Cents(150_000), available_retainer(balance, []).unwrap()).unwrap();
        assert_eq!(first, Cents(-150_000));
        let left = available_retainer(balance, [first]).unwrap();
        assert_eq!(left, Cents(50_000));
        assert_eq!(apply_retainer_amount(Cents(150_000), left), Ok(Cents(-50_000)));
        let none_left = available_retainer(balance, [first, Cents(-50_000)]).unwrap();
        assert_eq!(apply_retainer_amount(Cents(150_000), none_left), Err(ApplyRetainerRefused::NoRetainerAvailable));
    }
}
