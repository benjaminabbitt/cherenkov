//! A package the diff does not touch.

pub fn is_even(x: i32) -> bool {
    x % 2 == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_four_is_even() {
        assert!(is_even(4));
    }
}
