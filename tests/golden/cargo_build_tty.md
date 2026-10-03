## ✓ cargo: 1 warning(s)

warning: unused variable: `x`
 --> src/main.rs:1:16
  |
1 | fn main(){ let x = 5; println!("hi"); }
  |                ^ help: if this is intentional, prefix it with an underscore: `_x`
  |
  = note: `#[warn(unused_variables)]` (part of `#[warn(unused)]`) on by default

[tokencat: 590 -> 122 tokens (-79.3%) | saved ~$0.0019]
