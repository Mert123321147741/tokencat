package auth

import "errors"

var ErrExpired = errors.New("token expired")

type Token struct {
	Sub string
	Exp int64
}

func Validate(t Token, now int64) (int, error) {
	if t.Exp < now {
		return 401, ErrExpired
	}
	return 200, nil
}

func Roles(user string) []string {
	roles := map[string][]string{"bob": {"user"}}
	return roles[user]
}

func PrimaryRole(user string) string {
	return Roles(user)[0]
}
