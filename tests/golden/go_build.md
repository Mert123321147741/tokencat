# example.com/shop/util
util/util.go:4:25: undefined: undefinedThing
# example.com/shop/auth [example.com/shop/auth.test]
auth/auth.go:16:9: cannot use "200" (untyped string constant) as int value in return statement
FAIL	example.com/shop/auth [build failed]
FAIL	example.com/shop/util [build failed]
FAIL

util/util.go:4:25
```go
 2 | 
 3 | func Double(x int) int { return x * 2 }
>4 | func Bad() int { return undefinedThing }
```

auth/auth.go:16:9
```go
 12 | func Validate(t Token, now int64) (int, error) {
    | ⋮
 14 | 		return 401, ErrExpired
 15 | 	}
>16 | 	return "200", nil
 17 | }
 18 | 
```
