//! A package that uses the others: its tests reach `ws_core::double`.

pub fn run(x: i32) -> i32 {
    if ws_util::is_even(x) { ws_core::double(x) } else { x }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_run_doubles_an_even_number() {
        assert_eq!(run(4), 8);
    }

    #[test]
    fn test_run_keeps_an_odd_number() {
        assert_eq!(run(3), 3);
    }
}
