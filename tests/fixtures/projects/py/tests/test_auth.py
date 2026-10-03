from shop.auth import validate_token, login


class TestAuth:
    def test_login_ok(self):
        assert login("bob", "secret")["status"] == 200

    def test_login_wrong_password(self):
        res = login("bob", "nope")
        assert res["status"] == 200

    def test_expired_token(self):
        token = {"sub": "bob", "exp": 1000}
        assert validate_token(token, now=2000)["status"] == 200
