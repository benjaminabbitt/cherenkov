use std::num::{NonZero, NonZeroI64, NonZeroU32};

pub fn named_unsigned() -> NonZeroU32 {
    NonZeroU32::MIN
}
pub fn named_signed() -> NonZeroI64 {
    NonZeroI64::MIN
}
pub fn generic_unsigned() -> NonZero<u32> {
    NonZero::<u32>::MIN
}
pub fn generic_signed() -> NonZero<i64> {
    NonZero::<i64>::MIN
}
pub fn generic_qualified() -> std::num::NonZero<usize> {
    NonZero::<usize>::MIN
}
