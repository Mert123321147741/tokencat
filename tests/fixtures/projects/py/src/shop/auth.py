import time


class TokenExpired(Exception):
    pass


def validate_token(token, now=None):
    now = now or time.time()
    if token["exp"] < now:
        raise TokenExpired(f"token expired at {token['exp']}")
    return {"status": 200, "user": token["sub"]}


def login(user, password):
    if password != "secret":
        return {"status": 401}
    return {"status": 200, "token": {"sub": user, "exp": time.time() + 3600}}
