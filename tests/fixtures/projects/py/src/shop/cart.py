from dataclasses import dataclass, field


@dataclass
class Item:
    sku: str
    price: float
    qty: int = 1


@dataclass
class Cart:
    items: list = field(default_factory=list)

    def add(self, item):
        self.items.append(item)

    def total(self):
        return sum(i.price * i.qty for i in self.items)

    def apply_discount(self, code):
        rates = {"SAVE10": 0.10, "HALF": 0.5}
        rate = rates[code]
        return round(self.total() * (1 - rate), 2)

    def average_price(self):
        return self.total() / len(self.items)
