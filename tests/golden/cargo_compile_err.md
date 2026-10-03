## ✗ cargo: build failed with 2 error(s)

### 1. error[E0308]: mismatched types
at src/lib.rs:38:5
```
   |
36 | pub fn total(xs: &[u32]) -> u32 {
   |                             --- expected `u32` because of return type
37 |     let unused = 5;
38 |     xs.iter().map(|x| x * 2).collect::<Vec<u32>>()
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ expected `u32`, found `Vec<u32>`
   |
   = note: expected type `u32`
            found struct `Vec<u32>`
```

### 2. error[E0382]: borrow of moved value: `n`
at src/lib.rs:43:20
```
   |
41 | pub fn name_len(n: String) -> usize {
   |                 - move occurs because `n` has type `String`, which does not implement the `Copy` trait
42 |     let s = n;
   |             - value moved here
43 |     println!("{}", n);
   |                    ^ value borrowed here after move
   |
help: consider cloning the value if the performance cost is acceptable
   |
42 |     let s = n.clone();
   |              ++++++++
```

[tokencat: 431 -> 338 tokens (-21.6%) | saved ~$0.0004]
