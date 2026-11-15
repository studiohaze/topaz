size = 40
left = []
right = []
for row in range(size):
    for column in range(size):
        left.append((row * 3 + column * 5) % 17 - 8)
        right.append((row * 7 + column * 2) % 19 - 9)
checksum = 0
for round_number in range(8):
    product = []
    for row in range(size):
        for column in range(size):
            value = 0
            for inner in range(size):
                value += left[row * size + inner] * right[inner * size + column]
            product.append(value)
    for index in range(len(product)):
        checksum += product[index] * (index + 1) * (round_number + 1)
print(f"bench-v1:{checksum}")
