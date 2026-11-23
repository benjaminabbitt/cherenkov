//! The package a diff touches.

pub fn double(x: i32) -> i32 {
    x * 2
}

pub fn triple(x: i32) -> i32 {
    x * 3
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_double_of_three_is_six() {
        assert_eq!(double(3), 6);
    }

    #[test]
    fn test_triple_of_three_is_nine() {
        assert_eq!(triple(3), 9);
    }
}
