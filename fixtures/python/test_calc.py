from calc import count_up, is_small, total


def test_total():
    assert total(2, 3) == 5


def test_is_small():
    assert is_small(3)
    assert not is_small(12)


def test_count_up():
    assert count_up(4) == 4
