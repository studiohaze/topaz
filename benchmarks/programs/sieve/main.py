limit = 100000
composite = [False] * (limit + 1)
candidate = 2
while candidate * candidate <= limit:
    if not composite[candidate]:
        multiple = candidate * candidate
        while multiple <= limit:
            composite[multiple] = True
            multiple += candidate
    candidate += 1
count = 0
total = 0
for value in range(2, limit + 1):
    if not composite[value]:
        count += 1
        total += value
print(f"bench-v1:{count}:{total}")
