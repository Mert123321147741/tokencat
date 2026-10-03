## ✗ jest: 3 failed, 121 passed, 124 total

### 1. AuthService › rejects wrong password with 200
at src/auth.test.js:12:24
expect(received).toBe(expected) // Object.is equality

Expected: 200
Received: 401
```js
  10 |   it('rejects wrong password with 200', () => {
  11 |     const res = login('bob', 'nope');
> 12 |     expect(res.status).toBe(200);
     |                        ^
  13 |   });
  14 |
```

### 2. AuthService › accepts expired token
at src/auth.js:10:11
token expired at 1000
  at src/auth.test.js:16:17
```js
   8 |   const payload = decode(token);
   9 |   if (payload.exp < now) {
> 10 |     throw new UnauthorizedError(`token expired at ${payload.exp}`);
     |           ^
  11 |   }
  12 |   return { status: 200, user: payload.sub };
```

### 3. AuthService › returns user object
at src/auth.test.js:23:17
expect(received).toEqual(expected) // deep equality

- Expected  - 4
+ Received  + 1

  Object {
-   "roles": Array [
-     "admin",
-   ],
    "status": 200,
-   "user": "alice",
+   "user": "bob",
  }
```js
  21 |     console.log('debug: validating');
  22 |     const res = validateToken(enc({ sub: 'bob', exp: 5000 }), 2000);
> 23 |     expect(res).toEqual({ status: 200, user: 'alice', roles: ['admin'] });
     |                 ^
  24 |   });
  25 | });
```

[tokencat: 633 -> 510 tokens (-19.4%) | saved ~$0.0005]
