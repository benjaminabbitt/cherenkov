package calc

import "testing"

func TestTotal(t *testing.T) {
	if Total(2, 3) != 5 {
		t.Fatal("Total")
	}
}

func TestIsSmall(t *testing.T) {
	if !IsSmall(3) || IsSmall(12) {
		t.Fatal("IsSmall")
	}
}

func TestCountUp(t *testing.T) {
	if CountUp(4) != 4 {
		t.Fatal("CountUp")
	}
}

func TestSum(t *testing.T) {
	if Sum([]int{1, 2}) != 3 {
		t.Fatal("Sum")
	}
}
