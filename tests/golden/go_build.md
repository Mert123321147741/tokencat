## ✗ go: 2 build error(s), packages: 2 failed, 0 ok

### 1. build failed: example.com/shop/util
at util/util.go:4:25
util/util.go:4:25: undefined: undefinedThing
```go
 2 | 
 3 | func Double(x int) int { return x * 2 }
>4 | func Bad() int { return undefinedThing }
```

### 2. build failed: example.com/shop/auth
at auth/auth.go:16:9
auth/auth.go:16:9: cannot use "200" (untyped string constant) as int value in return statement
```go
 12 | func Validate(t Token, now int64) (int, error) {
    | ⋮
 14 | 		return 401, ErrExpired
 15 | 	}
>16 | 	return "200", nil
 17 | }
 18 | 
```
