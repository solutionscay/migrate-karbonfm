//! Integer division rounded half away from zero, as FileMaker Round() does
//! (BR-SALES-01, BR-SALES-02, BR-IMPORT-01).

/// `n / d` rounded to the nearest integer, ties away from zero. `d` must be positive.
pub fn div_round_half_away(n: i128, d: i128) -> i128 {
    assert!(d > 0, "divisor must be positive");
    let q = n / d; // truncates toward zero
    let r = n % d; // same sign as n
    if 2 * r.abs() >= d {
        q + n.signum()
    } else {
        q
    }
}

#[cfg(test)]
mod tests {
    use super::div_round_half_away as r;

    #[test]
    fn exact() {
        assert_eq!(r(0, 7), 0);
        assert_eq!(r(20, 10), 2);
        assert_eq!(r(-20, 10), -2);
    }

    #[test]
    fn ties_go_away_from_zero() {
        assert_eq!(r(5, 10), 1);
        assert_eq!(r(-5, 10), -1);
        assert_eq!(r(15, 10), 2);
        assert_eq!(r(-15, 10), -2);
        assert_eq!(r(25, 10), 3); // not banker's rounding (which gives 2)
        assert_eq!(r(-25, 10), -3);
    }

    #[test]
    fn below_and_above_the_tie() {
        assert_eq!(r(4, 10), 0);
        assert_eq!(r(6, 10), 1);
        assert_eq!(r(-4, 10), 0);
        assert_eq!(r(-6, 10), -1);
        assert_eq!(r(49_999, 10_000), 5);
        assert_eq!(r(4_999, 10_000), 0);
    }

    #[test]
    fn odd_divisor_has_no_tie() {
        assert_eq!(r(1, 3), 0);
        assert_eq!(r(2, 3), 1);
        assert_eq!(r(-2, 3), -1);
    }

    #[test]
    #[should_panic]
    fn zero_divisor_panics() {
        r(1, 0);
    }
}
