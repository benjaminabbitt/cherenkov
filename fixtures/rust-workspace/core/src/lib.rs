//! The package a diff touches.

pub fn double(x: i32) -> i32 {
    x * 2
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_double_of_three_is_six() {
        assert_eq!(double(3), 6);
    }
}
