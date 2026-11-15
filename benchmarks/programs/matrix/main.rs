fn main() {
    let size = 40;
    let mut left = Vec::new();
    let mut right = Vec::new();
    for row in 0..size {
        for column in 0..size {
            left.push(((row * 3 + column * 5) % 17) as i64 - 8);
            right.push(((row * 7 + column * 2) % 19) as i64 - 9);
        }
    }
    let mut checksum = 0i64;
    for round in 0..8 {
        let mut product = Vec::new();
        for row in 0..size {
            for column in 0..size {
                let mut value = 0;
                for inner in 0..size {
                    value += left[row * size + inner] * right[inner * size + column];
                }
                product.push(value);
            }
        }
        for index in 0..product.len() {
            checksum += product[index] * (index + 1) as i64 * (round + 1);
        }
    }
    println!("bench-v1:{checksum}");
}
