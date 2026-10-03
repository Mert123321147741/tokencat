import pytest
from shop.cart import Cart, Item


def test_discount_unknown_code():
    cart = Cart()
    cart.add(Item("book", 12.5, 2))
    assert cart.apply_discount("SAVE20") == 22.5


def test_average_empty_cart():
    cart = Cart()
    assert cart.average_price() == 0


def test_total_with_items():
    cart = Cart()
    cart.add(Item("pen", 1.5, 4))
    cart.add(Item("ink", 3.0))
    print("cart items:", cart.items)
    assert cart.total() == 9.5


def test_dict_compare():
    expected = {"sku": "pen", "price": 1.5, "qty": 4, "tags": ["office", "write"]}
    actual = {"sku": "pen", "price": 1.25, "qty": 4, "tags": ["office", "writing"]}
    assert actual == expected
