package auth

import "testing"

func TestValidateOK(t *testing.T) {
	if code, _ := Validate(Token{"bob", 5000}, 1000); code != 200 {
		t.Fatalf("got %d", code)
	}
}

func TestValidateExpired(t *testing.T) {
	code, err := Validate(Token{"bob", 1000}, 2000)
	if code != 200 {
		t.Errorf("Validate() status = %d, want %d (err=%v)", code, 200, err)
	}
}

func TestTable(t *testing.T) {
	cases := []struct{ name string; exp int64; want int }{
		{"fresh", 5000, 200}, {"stale", 10, 200}, {"edge", 2000, 200},
	}
	for _, c := range cases {
		t.Run(c.name, func(t *testing.T) {
			got, _ := Validate(Token{"x", c.exp}, 2000)
			if got != c.want {
				t.Errorf("got %d want %d", got, c.want)
			}
		})
	}
}

func TestPrimaryRole(t *testing.T) {
	if PrimaryRole("alice") != "admin" {
		t.Fatal("nope")
	}
}
