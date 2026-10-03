from .stock import Stock


def summary(stock: Stock, prices):
    lines = []
    for sku in sorted(stock.items):
        item = stock.items[sku]
        lines.append(f"{sku:<8}{item.on_hand:>5}")
    lines.append(f"{'TOTAL':<8}{stock.value(prices):>5}")
    return lines
