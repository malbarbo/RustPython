pub mod rounding_modes {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum RoundingMode {
        Nearest,
    }
}

pub mod num {
    pub mod conversion {
        pub mod traits {
            use crate::rounding_modes::RoundingMode;
            use core::cmp::Ordering;

            pub trait RoundingInto<T> {
                fn rounding_into(self, rm: RoundingMode) -> (T, Ordering);
            }
        }
    }

    pub mod basic {
        pub mod floats {
            pub trait PrimitiveFloat: Copy {
                fn is_negative_zero(self) -> bool;
            }
            impl PrimitiveFloat for f32 {
                fn is_negative_zero(self) -> bool {
                    self.to_bits() == 0x8000_0000
                }
            }
            impl PrimitiveFloat for f64 {
                fn is_negative_zero(self) -> bool {
                    self.to_bits() == 0x8000_0000_0000_0000
                }
            }
        }
    }
}
