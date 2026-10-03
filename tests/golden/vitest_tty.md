## ✗ vitest: 3 failed | 101 passed (104)

### 1. src/auth.test.ts > AuthService > rejects expired token
at src/auth.test.ts:11:24
AssertionError: expected 401 to be 200 // Object.is equality

- Expected
+ Received

- 200
+ 401
```ts
 9|   it('rejects expired token', () => {
10|     const res = login('bob', 'nope');
11|     expect(res.status).toBe(200);
  |                        ^
12|   });
13|
```

### 2. src/auth.test.ts > AuthService > accepts expired token
at src/auth.ts:5:11
Error: token expired at 1000
  at src/auth.test.ts:15:17
```ts
3| export function validateToken(token: { sub: string; exp: number }, now…
4|   if (token.exp < now) {
5|     throw new UnauthorizedError(`token expired at ${token.exp}`);
 |           ^
6|   }
7|   return { status: 200, user: token.sub };
```

### 3. src/auth.test.ts > AuthService > returns user object
at src/auth.test.ts:22:17
AssertionError: expected { status: 200, user: 'bob' } to deeply equal { status: 200, user: 'alice', …(1) }

- Expected
+ Received

  {
-   "roles": [
-     "admin",
-   ],
    "status": 200,
-   "user": "alice",
+   "user": "bob",
  }
```ts
20|     console.log('debug: validating');
21|     const res = validateToken({ sub: 'bob', exp: 5000 }, 2000);
22|     expect(res).toEqual({ status: 200, user: 'alice', roles: ['admin']…
  |                 ^
23|   });
24| });
```

[tokencat: 799 -> 551 tokens (-31.0%) | saved ~$0.0010]
