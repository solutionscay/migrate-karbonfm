//! Property tests for the money edges and rounding (S-03).

use karbon_money::sales::{line_amount, tax, totals, Line};
use karbon_money::{Cents, ParseError, Qty, QTY_SCALE};
use proptest::prelude::*;

/// Money amounts a form or the seed can realistically hold (up to $10 billion),
/// so products with quantities and rates stay inside i64.
fn amount() -> impl Strategy<Value = i64> {
    -1_000_000_000_000i64..=1_000_000_000_000
}

/// Quantities up to 100,000 at 4 decimals.
fn qty_units() -> impl Strategy<Value = i64> {
    -1_000_000_000i64..=1_000_000_000
}

/// Checks `rounded` is `exact / divisor` rounded half away from zero.
fn is_half_away(rounded: i64, exact: i128, divisor: i128) -> bool {
    let scaled = i128::from(rounded) * divisor;
    let diff = (scaled - exact).abs();
    2 * diff < divisor || (2 * diff == divisor && scaled.abs() > exact.abs())
}

proptest! {
    #[test]
    fn display_round_trips(c in any::<i64>()) {
        prop_assert_eq!(Cents::parse(&Cents(c).to_string()), Ok(Cents(c)));
    }

    #[test]
    fn input_value_round_trips(c in any::<i64>()) {
        prop_assert_eq!(Cents::parse(&Cents(c).to_input_value()), Ok(Cents(c)));
    }

    /// Builds "-$1,234.5"-style text from parts and checks the parsed cents.
    #[test]
    fn parses_composed_text(
        dollars in 0u64..100_000_000_000,
        frac in prop_oneof![Just(None), (0u64..10).prop_map(|d| Some((d, 1))), (0u64..100).prop_map(|d| Some((d, 2)))],
        negative: bool,
        dollar_sign: bool,
        sign_first: bool,
        grouped: bool,
        pad: bool,
    ) {
        let whole = if grouped {
            Cents((dollars * 100) as i64).to_string().trim_start_matches('$').split('.').next().unwrap().to_string()
        } else {
            dollars.to_string()
        };
        let frac_text = match frac {
            None => String::new(),
            Some((d, 1)) => format!(".{d}"),
            Some((d, _)) => format!(".{d:02}"),
        };
        let sign = if negative { "-" } else { "" };
        let dollar = if dollar_sign { "$" } else { "" };
        let prefix = if sign_first { format!("{sign}{dollar}") } else { format!("{dollar}{sign}") };
        let text = if pad { format!("  {prefix}{whole}{frac_text}\t") } else { format!("{prefix}{whole}{frac_text}") };

        let frac_cents = match frac {
            None => 0,
            Some((d, 1)) => d * 10,
            Some((d, _)) => d,
        };
        let magnitude = (dollars * 100 + frac_cents) as i64;
        let expected = if negative { -magnitude } else { magnitude };
        prop_assert_eq!(Cents::parse(&text), Ok(Cents(expected)), "{:?}", text);
    }

    #[test]
    fn more_than_two_decimals_is_always_refused(dollars in 0u64..1_000_000, extra in "[0-9]{3,8}") {
        let text = format!("{dollars}.{extra}");
        prop_assert_eq!(Cents::parse(&text), Err(ParseError::TooManyDecimals { max: 2 }));
    }

    /// Arbitrary text never panics, and whatever parses formats back to itself.
    #[test]
    fn junk_never_panics(text in "\\PC{0,24}") {
        if let Ok(c) = Cents::parse(&text) {
            prop_assert_eq!(Cents::parse(&c.to_string()), Ok(c));
        }
        let _ = Qty::parse(&text);
    }

    /// Near-valid text: whatever parses means the same as its digits with the
    /// `$` and commas removed, read by a naive reference parser.
    #[test]
    fn near_valid_text_parses_to_its_digits(text in "[-+$0-9,. ]{0,14}") {
        if let Ok(c) = Cents::parse(&text) {
            let bare: String = text.trim().chars().filter(|ch| *ch != '$' && *ch != ',').collect();
            let (negative, digits) = match bare.strip_prefix('-') {
                Some(rest) => (true, rest),
                None => (false, bare.strip_prefix('+').unwrap_or(&bare)),
            };
            let (whole, frac) = digits.split_once('.').unwrap_or((digits, ""));
            let frac = format!("{frac:0<2}");
            let magnitude: i64 = format!("0{whole}{frac}").parse().unwrap();
            prop_assert_eq!(c.0, if negative { -magnitude } else { magnitude }, "{:?}", text);
        }
    }

    #[test]
    fn qty_round_trips(units in any::<i64>()) {
        let q = Qty::from_scaled(units);
        prop_assert_eq!(Qty::parse(&q.to_string()), Ok(q));
        prop_assert_eq!(Qty::parse(&q.to_input_value()), Ok(q));
    }

    #[test]
    fn line_amount_rounds_half_away_from_zero(q in qty_units(), p in amount()) {
        let a = line_amount(Some(Qty::from_scaled(q)), Cents(p)).unwrap();
        prop_assert!(is_half_away(a.0, i128::from(q) * i128::from(p), i128::from(QTY_SCALE)));
    }

    #[test]
    fn line_amount_is_symmetric(q in qty_units(), p in amount()) {
        let a = line_amount(Some(Qty::from_scaled(q)), Cents(p)).unwrap();
        prop_assert_eq!(line_amount(Some(Qty::from_scaled(-q)), Cents(p)).unwrap(), -a);
        prop_assert_eq!(line_amount(Some(Qty::from_scaled(q)), Cents(-p)).unwrap(), -a);
    }

    #[test]
    fn whole_quantities_are_exact(n in -100_000i64..=100_000, p in amount()) {
        prop_assert_eq!(line_amount(Qty::from_whole(n), Cents(p)), Ok(Cents(n * p)));
    }

    #[test]
    fn tax_rounds_half_away_from_zero(t in amount(), bp in 0i64..=10_000) {
        let x = tax(Cents(t), bp).unwrap();
        prop_assert!(is_half_away(x.0, i128::from(t) * i128::from(bp), 10_000));
    }

    #[test]
    fn tax_is_monotonic(a in amount(), b in amount(), bp in 0i64..=10_000) {
        let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
        prop_assert!(tax(Cents(lo), bp).unwrap() <= tax(Cents(hi), bp).unwrap());
    }

    #[test]
    fn totals_add_up(
        lines in prop::collection::vec((prop::option::of(0i64..=1_000_000), -10_000_000i64..=10_000_000, any::<bool>(), any::<bool>()), 0..20),
        bp in 0i64..=2_000,
        ship in 0i64..=100_000,
        paid in 0i64..=100_000_000,
    ) {
        let lines: Vec<Line> = lines
            .into_iter()
            .map(|(q, p, taxable, retainer)| Line { qty: q.map(Qty::from_scaled), unit_price: Cents(p), taxable, retainer })
            .collect();
        let t = totals(&lines, bp, Cents(ship), Cents(paid)).unwrap();
        let amounts: Vec<Cents> = lines.iter().map(|l| l.amount().unwrap()).collect();
        prop_assert_eq!(t.subtotal, amounts.iter().copied().sum::<Cents>());
        prop_assert_eq!(t.taxable_subtotal, lines.iter().zip(&amounts).filter(|(l, _)| l.taxable).map(|(_, a)| *a).sum::<Cents>());
        prop_assert_eq!(t.retainer, lines.iter().zip(&amounts).filter(|(l, _)| l.retainer).map(|(_, a)| *a).sum::<Cents>());
        prop_assert_eq!(t.total_tax, tax(t.taxable_subtotal, bp).unwrap());
        prop_assert_eq!(t.total_amt, t.subtotal + t.total_tax + Cents(ship));
        prop_assert_eq!(t.total_amt - t.balance, Cents(paid));
    }
}
