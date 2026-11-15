fn fibonacci(n: i64) -> i64 {
    if n < 2 {
        return n;
    }
    fibonacci(n - 1) + fibonacci(n - 2)
}

fn main() {
    println!("bench-v1:{}", fibonacci(26));
}
