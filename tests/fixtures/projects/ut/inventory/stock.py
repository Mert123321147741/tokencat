"""Stock levels and reorder rules for a small warehouse."""

from dataclasses import dataclass


class OutOfStock(Exception):
    pass


@dataclass
class Item:
    sku: str
    on_hand: int
    reorder_at: int = 5


class Stock:
    def __init__(self, items=()):
        self.items = {i.sku: i for i in items}

    def take(self, sku, qty):
        item = self.items[sku]
        if qty > item.on_hand:
            raise OutOfStock(f"{sku}: wanted {qty}, have {item.on_hand}")
        item.on_hand -= qty
        return item.on_hand

    def needs_reorder(self):
        # Items at or below their reorder point, by SKU.
        return sorted(sku for sku, i in self.items.items() if i.on_hand < i.reorder_at)

    def value(self, prices):
        total = 0
        for sku, item in self.items.items():
            total += prices[sku] * item.on_hand
        return total
