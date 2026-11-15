fn main() {
    let limit = 100000;
    let mut composite = vec![false; limit + 1];
    let mut candidate = 2;
    while candidate * candidate <= limit {
        if !composite[candidate] {
            let mut multiple = candidate * candidate;
            while multiple <= limit {
                composite[multiple] = true;
                multiple += candidate;
            }
        }
        candidate += 1;
    }
    let mut count = 0i64;
    let mut total = 0i64;
    for value in 2..=limit {
        if !composite[value] {
            count += 1;
            total += value as i64;
        }
    }
    println!("bench-v1:{count}:{total}");
}
