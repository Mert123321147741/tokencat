pub fn parse_port(s: &str) -> u16 {
    s.trim().parse().unwrap()
}

pub fn ratio(a: u32, b: u32) -> u32 {
    a / b
}

pub fn greet(name: &str) -> String {
    format!("Hello, {name}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn greets() {
        assert_eq!(greet("Ada"), "Hello, Ada!");
    }

    #[test]
    fn parses_port() {
        assert_eq!(parse_port("80a"), 80);
    }

    #[test]
    fn ratio_by_zero() {
        assert_eq!(ratio(1, 0), 0);
    }

    #[test]
    fn ok_one() { assert!(true); }
}
