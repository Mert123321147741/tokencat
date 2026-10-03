## ✗ unittest: 3 failed, 1 error, 1 skipped (51 tests in 0.004s)

### 1. tests.test_stock.TakeTests.test_unknown_sku (error)
at inventory/stock.py:22
KeyError: 'screw'
  at tests/test_stock.py:26 in test_unknown_sku: self.assertEqual(self.stock.take("screw", 1), 0)
```python
 20 | 
 21 |     def take(self, sku, qty):
>22 |         item = self.items[sku]
 23 |         if qty > item.on_hand:
 24 |             raise OutOfStock(f"{sku}: wanted {qty}, have {item.on_hand}")
```

### 2. tests.test_stock.ReorderTests.test_low_items
at tests/test_stock.py:41
AssertionError: Lists differ: ['washer'] != ['nut', 'washer']

First differing element 0:
'washer'
'nut'

Second list contains 1 additional elements.
First extra element 1:
'washer'

- ['washer']
+ ['nut', 'washer']
```python
 39 |     def test_low_items(self):
 40 |         stock = make_stock()
>41 |         self.assertEqual(stock.needs_reorder(), ["nut", "washer"])
 42 | 
 43 |     def test_reorder_point_per_item(self):
```

### 3. tests.test_stock.ReorderTests.test_reorder_point_per_item
at tests/test_stock.py:45
AssertionError: Lists differ: [] != ['a']

Second list contains 1 additional elements.
First extra element 0:
'a'

- []
+ ['a']
```python
 43 |     def test_reorder_point_per_item(self):
 44 |         stock = Stock([Item("a", 3, reorder_at=3), Item("b", 4, reorder_at=3)])
>45 |         self.assertEqual(stock.needs_reorder(), ["a"])
 46 | 
 47 | 
```

### 4. tests.test_stock.TakeTests.test_take_in_steps (qty=3)
at tests/test_stock.py:31
AssertionError: 4 != 5
```python
 28 |     def test_take_in_steps(self):
 29 |         for qty, left in [(1, 9), (2, 7), (3, 5)]:
 30 |             with self.subTest(qty=qty):
>31 |                 self.assertEqual(self.stock.take("bolt", qty), left)
 32 | 
 33 | 
```

[tokencat: 1,747 -> 680 tokens (-61.1%) | saved ~$0.0043]
