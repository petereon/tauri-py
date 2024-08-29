from typing import Optional

def greet(name: str) -> str:
    return f"Hello, {name}! You have been greeted from Python!"

def sum(a: int, b: int) -> int:
    return a + b


def division(a: int, b: int) -> Optional[float]:
    if b == 0:
        return None
    return a / b
