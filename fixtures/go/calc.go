package calc

func log(_ int) {}

func Total(a, b int) int {
	s := a + b
	log(s)
	return s
}

func IsSmall(n int) bool {
	if n < 10 {
		return true
	}
	return false
}

func CountUp(n int) int {
	i := 0
	for i < n {
		i = i + 1
	}
	return i
}

func Sum(xs []int) int {
	t := 0
	for j := 0; j < len(xs); j++ {
		t += xs[j]
	}
	return t
}
