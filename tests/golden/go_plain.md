## ✗ go: 3 failed, packages: 1 failed, 1 ok

### 1. TestValidateExpired
at auth/auth_test.go:14
Validate() status = 401, want 200 (err=token expired)
```go
 11 | func TestValidateExpired(t *testing.T) {
 12 | 	code, err := Validate(Token{"bob", 1000}, 2000)
 13 | 	if code != 200 {
>14 | 		t.Errorf("Validate() status = %d, want %d (err=%v)", code, 200, err)
 15 | 	}
 16 | }
```

### 2. TestTable/stale
at auth/auth_test.go:26
got 401 want 200
```go
 18 | func TestTable(t *testing.T) {
    | ⋮
 24 | 			got, _ := Validate(Token{"x", c.exp}, 2000)
 25 | 			if got != c.want {
>26 | 				t.Errorf("got %d want %d", got, c.want)
 27 | 			}
 28 | 		})
```

### 3. TestPrimaryRole
at auth/auth.go:25
panic: runtime error: index out of range [0] with length 0
  at auth/auth_test.go:33 in auth.TestPrimaryRole
```go
 23 | 
 24 | func PrimaryRole(user string) string {
>25 | 	return Roles(user)[0]
 26 | }
```

[tokencat: 500 -> 373 tokens (-25.4%) | saved ~$0.0005]
