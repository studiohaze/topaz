const size = 40;
const left: number[] = [];
const right: number[] = [];
for (let row = 0; row < size; row += 1) {
    for (let column = 0; column < size; column += 1) {
        left.push((row * 3 + column * 5) % 17 - 8);
        right.push((row * 7 + column * 2) % 19 - 9);
    }
}
let checksum = 0;
for (let round = 0; round < 8; round += 1) {
    const product: number[] = [];
    for (let row = 0; row < size; row += 1) {
        for (let column = 0; column < size; column += 1) {
            let value = 0;
            for (let inner = 0; inner < size; inner += 1) {
                value += left[row * size + inner] * right[inner * size + column];
            }
            product.push(value);
        }
    }
    for (let index = 0; index < product.length; index += 1) {
        checksum += product[index] * (index + 1) * (round + 1);
    }
}
console.log(`bench-v1:${checksum}`);
