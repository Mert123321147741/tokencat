## ✗ pytest: 6 failed, 91 passed in 0.11s

### 1. tests/test_auth.py::TestAuth::test_login_wrong_password
at tests/test_auth.py:10
assert 401 == 200
```python
  8 |     def test_login_wrong_password(self):
  9 |         res = login("bob", "nope")
>10 |         assert res["status"] == 200
 11 | 
 12 |     def test_expired_token(self):
```

### 2. tests/test_auth.py::TestAuth::test_expired_token
at src/shop/auth.py:11
shop.auth.TokenExpired: token expired at 1000
  at tests/test_auth.py:14 in test_expired_token: assert validate_token(token, now=2000)["status"] == 200
```python
  8 | def validate_token(token, now=None):
  9 |     now = now or time.time()
 10 |     if token["exp"] < now:
>11 |         raise TokenExpired(f"token expired at {token['exp']}")
 12 |     return {"status": 200, "user": token["sub"]}
 13 | 
```

### 3. tests/test_cart.py::test_discount_unknown_code
at src/shop/cart.py:23
KeyError: 'SAVE20'
  at tests/test_cart.py:8 in test_discount_unknown_code: assert cart.apply_discount("SAVE20") == 22.5
```python
 21 |     def apply_discount(self, code):
 22 |         rates = {"SAVE10": 0.10, "HALF": 0.5}
>23 |         rate = rates[code]
 24 |         return round(self.total() * (1 - rate), 2)
 25 | 
```

### 4. tests/test_cart.py::test_average_empty_cart
at src/shop/cart.py:27
ZeroDivisionError: division by zero
  at tests/test_cart.py:13 in test_average_empty_cart: assert cart.average_price() == 0
```python
 25 | 
 26 |     def average_price(self):
>27 |         return self.total() / len(self.items)
```

### 5. tests/test_cart.py::test_total_with_items
at tests/test_cart.py:21
AssertionError: assert 9.0 == 9.5
 +  where 9.0 = total()
 +    where total = Cart(items=[Item(sku='pen', price=1.5, qty=4), Item(sku='ink', price=3.0, qty=1)]).total
--- captured stdout call ---
cart items: [Item(sku='pen', price=1.5, qty=4), Item(sku='ink', price=3.0, qty=1)]
```python
 16 | def test_total_with_items():
    | ⋮
 19 |     cart.add(Item("ink", 3.0))
 20 |     print("cart items:", cart.items)
>21 |     assert cart.total() == 9.5
 22 | 
 23 | 
```

### 6. tests/test_cart.py::test_dict_compare
at tests/test_cart.py:27
AssertionError: assert {'sku': 'pen'...', 'writing']} == {'sku': 'pen'...ce', 'write']}

  Omitting 2 identical items, use -vv to show
  Differing items:
  {'price': 1.25} != {'price': 1.5}
  {'tags': ['office', 'writing']} != {'tags': ['office'[0m

  ...Full output truncated (2 lines hidden), use '-vv' to show
```python
 24 | def test_dict_compare():
 25 |     expected = {"sku": "pen", "price": 1.5, "qty": 4, "tags": ["office", "write"]}
 26 |     actual = {"sku": "pen", "price": 1.25, "qty": 4, "tags": ["office", "writing"]}
>27 |     assert actual == expected
```

[tokencat: 1,683 -> 1,083 tokens (-35.7%) | saved ~$0.0024]
