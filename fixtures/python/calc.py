def log(msg):
    pass


def total(a: int, b: int) -> int:
    s = a + b
    log(s)
    return s


def is_small(n: int) -> bool:
    if n < 10:
        return True
    return False


def count_up(n: int) -> int:
    i = 0
    while i < n:
        i = i + 1
    return i


def chained(a, b, c):
    return a < b < c
