## ✗ cargo: 3 failed, 1 passed

### 1. tests::greets
at src/lib.rs:19:9
assertion `left == right` failed
  left: "Hello, Ada"
 right: "Hello, Ada!"
```rust
 17 |     #[test]
 18 |     fn greets() {
>19 |         assert_eq!(greet("Ada"), "Hello, Ada!");
 20 |     }
 21 | 
```

### 2. tests::ratio_by_zero
at src/lib.rs:6:5
attempt to divide by zero
  at src/lib.rs:29:20 in rs::tests::ratio_by_zero
```rust
 4 | 
 5 | pub fn ratio(a: u32, b: u32) -> u32 {
>6 |     a / b
 7 | }
 8 | 
```

### 3. tests::parses_port
at src/lib.rs:2:22
called `Result::unwrap()` on an `Err` value: ParseIntError { kind: InvalidDigit }
  at src/lib.rs:24:20 in rs::tests::parses_port
```rust
 1 | pub fn parse_port(s: &str) -> u16 {
>2 |     s.trim().parse().unwrap()
 3 | }
 4 | 
 5 | pub fn ratio(a: u32, b: u32) -> u32 {
```

[tokencat: 1,875 -> 347 tokens (-81.5%) | saved ~$0.0061]
