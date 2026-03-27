use core::cmp::Ordering;

use malachite_base::num::conversion::traits::RoundingInto;
use malachite_base::rounding_modes::RoundingMode;
use num_bigint::{BigInt, BigUint, Sign};
use num_rational::BigRational;
use num_traits::{Signed, ToPrimitive, Zero};

pub struct Rational(BigRational);

impl Rational {
    pub fn from_integers_ref(numerator: &BigInt, denominator: &BigInt) -> Self {
        Rational(BigRational::new(numerator.clone(), denominator.clone()))
    }

    pub fn into_numerator_and_denominator(self) -> (BigUint, BigUint) {
        let r = self.0;
        let numer = r.numer().magnitude().clone();
        let denom = r.denom().magnitude().clone();
        (numer, denom)
    }
}

impl TryFrom<f64> for Rational {
    type Error = ();

    fn try_from(value: f64) -> Result<Self, Self::Error> {
        if value.is_finite() {
            BigRational::from_float(value).map(Rational).ok_or(())
        } else {
            Err(())
        }
    }
}

impl RoundingInto<f64> for Rational {
    fn rounding_into(self, _rm: RoundingMode) -> (f64, Ordering) {
        match self.0.to_f64() {
            Some(val) if val.is_finite() => {
                match BigRational::from_float(val) {
                    Some(approx) => (val, self.0.cmp(&approx)),
                    None => (val, Ordering::Equal),
                }
            }
            _ => {
                if self.0.is_zero() {
                    (0.0, Ordering::Equal)
                } else if self.0.is_positive() {
                    (f64::MAX, Ordering::Less)
                } else {
                    (f64::MIN, Ordering::Greater)
                }
            }
        }
    }
}
