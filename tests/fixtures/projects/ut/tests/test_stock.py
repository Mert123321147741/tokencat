import unittest

from inventory.report import summary
from inventory.stock import Item, OutOfStock, Stock


def make_stock():
    return Stock([Item("bolt", 10), Item("nut", 5), Item("washer", 2, reorder_at=3)])


class TakeTests(unittest.TestCase):
    def setUp(self):
        self.stock = make_stock()

    def test_take_reduces_on_hand(self):
        self.assertEqual(self.stock.take("bolt", 3), 7)

    def test_take_everything(self):
        self.assertEqual(self.stock.take("nut", 5), 0)

    def test_take_too_many(self):
        with self.assertRaises(OutOfStock):
            self.stock.take("washer", 3)

    def test_unknown_sku(self):
        self.assertEqual(self.stock.take("screw", 1), 0)

    def test_take_in_steps(self):
        for qty, left in [(1, 9), (2, 7), (3, 5)]:
            with self.subTest(qty=qty):
                self.assertEqual(self.stock.take("bolt", qty), left)


class ReorderTests(unittest.TestCase):
    def test_nothing_low(self):
        stock = Stock([Item("bolt", 10), Item("nut", 9)])
        self.assertEqual(stock.needs_reorder(), [])

    def test_low_items(self):
        stock = make_stock()
        self.assertEqual(stock.needs_reorder(), ["nut", "washer"])

    def test_reorder_point_per_item(self):
        stock = Stock([Item("a", 3, reorder_at=3), Item("b", 4, reorder_at=3)])
        self.assertEqual(stock.needs_reorder(), ["a"])


class ReportTests(unittest.TestCase):
    def test_summary(self):
        lines = summary(make_stock(), {"bolt": 2, "nut": 1, "washer": 4})
        self.assertEqual(
            lines,
            ["bolt       10", "nut         5", "washer      2", "TOTAL      33"],
        )

    def test_summary_missing_price(self):
        with self.assertRaises(KeyError):
            summary(make_stock(), {"bolt": 2})

    @unittest.skip("needs the pricing service")
    def test_live_prices(self):
        pass


class SkuFormatTests(unittest.TestCase):
    pass


for _n in range(40):
    def _check(self, n=_n):
        stock = Stock([Item(f"sku{n}", n + 1)])
        self.assertEqual(stock.take(f"sku{n}", 1), n)

    setattr(SkuFormatTests, f"test_sku_{_n:02d}", _check)


if __name__ == "__main__":
    unittest.main()
