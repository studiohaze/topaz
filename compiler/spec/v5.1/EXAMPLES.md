# Topaz v5.1 Examples Library

Status: v5.1 locked. Mirrors `SPEC.md` v5.1.

This file holds canonical Topaz v5.1 worked examples. Every example below must:

1. Use only forms documented in `SPEC.md`.
2. Declare which SPEC sections it exercises.
3. Avoid speculative or non-canonical syntax.

If `SPEC.md` changes, examples must be updated or removed. An example whose
cited section no longer covers its forms is rejected.

## Reference Discipline

Each example follows this format:

```
### N. Title

Uses: §N, §M, ...

\`\`\`topaz
(code)
\`\`\`

Notes: (optional, one line)
```

`Uses` lists the primary exercised sections only. Baseline forms — string,
integer, float, boolean, null, and unit literals (§1); layout and statement
separation (§1a); `let` / `let mut` / `const` bindings (§4); block
expressions, function call, and `return` (§5); and the standard display call
`print(value: string) -> ()` (§22.2) when used only with a statically string
argument — are implicit in every example and are not re-cited. An example must
still avoid forms outside its cited sections plus the baseline.

**Snippet tier.** An entry marked `Snippet` is a syntax demonstration
that intentionally uses ambient names. Every snippet declares its ambient
names in an `Assumes:` line with their types; a snippet may not use
ambient names beyond its `Assumes:` line. Entries without the `Snippet`
mark are complete examples and must be self-contained.

## A. Minimal Forms

### A.1 Hello world

Uses: §1, §5, §7

```topaz
function greet(name: string) -> string {
    return "Hello, {name}!"
}

print(greet("Topaz"))
```

### A.2 Variable bindings

Uses: §1, §4

```topaz
const PI = 3.14159
let name = "Topaz"
let mut counter = 0
counter = counter + 1
```

### A.3 Function with explicit return

Uses: §2, §7

```topaz
function add(a: int, b: int) -> int {
    return a + b
}
```

### A.4 Last-expression return

Uses: §2, §5, §7

```topaz
function square(x: int) -> int {
    x * x
}
```

## B. Control Flow

### B.1 `if` / `else` as expression (Snippet)

Uses: §2, §5

Assumes: `hour: int`

```topaz
let greeting = if hour < 12 { "morning" }
               else if hour < 18 { "afternoon" }
               else { "evening" }
```

### B.2 `match` with wildcard (Snippet)

Uses: §5, §6

Assumes: `status: int`

```topaz
let label = match status {
    case 200 => "ok"
    case 404 => "not found"
    case _ => "other"
}
```

### B.3 `match` with type pattern

Uses: §1, §3, §5, §6, §7

```topaz
function describe(value: int | string | null) -> string {
    match value {
        case n: int => "number: {n}"
        case s: string => "string: {s}"
        case null => "no value"
    }
}
```

### B.4 `match` with guard (Snippet)

Uses: §2, §3, §5, §6

Assumes: `amount: int | string`

```topaz
let kind = match amount {
    case n: int if n > 0 => "positive"
    case n: int if n < 0 => "negative"
    case _ => "zero or non-numeric"
}
```

### B.5 `for` loop producing values

Uses: §2, §5, §10

```topaz
let doubled = for x in 1..5 { x * 2 }
```

### B.6 Range pattern in `match`

Uses: §5, §6, §7, §10

```topaz
function grade(score: int) -> string {
    match score {
        case 90..100 => "A"
        case 80..<90 => "B"
        case 70..<80 => "C"
        case _ => "F"
    }
}
```

## C. Types

### C.1 Type alias

Uses: §3, §4

```topaz
type UserId = int

let id: UserId = 42
```

### C.2 Literal union type

Uses: §3, §5, §6, §7

```topaz
type TrafficLight = "red" | "yellow" | "green"

function describe(color: TrafficLight) -> string {
    match color {
        case "red" => "stop"
        case "yellow" => "slow"
        case "green" => "go"
    }
}
```

### C.3 Nullable union

Uses: §1, §3, §4, §12

```topaz
let name: string | null = null
let display = name ?? "guest"
```

### C.4 Generic types

Uses: §3, §4, §9, §22

```topaz
let scores: Array<int> = [80, 90, 100]

let mut users: Map<string, int> = Map.new()
users.insert("alice", 1)
```

### C.5 Function type ascription

Uses: §2, §3, §4, §7

```topaz
let predicate: (int) -> bool = x => x > 0
```

## D. Records and Collections

### D.1 Record literal and field access

Uses: §5, §8

```topaz
let user = {
    name: "Alice",
    age: 30,
    city: "Seoul"
}

print(user.name)
```

### D.2 Record update

Uses: §8

```topaz
let user = { name: "Alice", age: 30 }
let older = user{ age: 31 }
```

### D.3 Array creation and mutation

Uses: §4, §9, §22

```topaz
let mut numbers: Array<int> = [1, 2, 3]
numbers.push(4)
```

### D.4 Map insertion and key membership

Uses: §2, §9, §22

```topaz
let mut prices: Map<string, int> = Map.new()
prices.insert("apple", 100)

let hasApple = "apple" in prices.keys
```

Notes: `prices.keys` is an insertion-order `Array<string>` snapshot
(§22.2); later mutation does not change an already produced keys array.

### D.5 Non-faulting reads with `get`

Uses: §4, §9, §13a, §22

```topaz
let mut prices: Map<string, int> = Map.new()
prices.insert("apple", 100)

let apple = prices.get("apple")      // Some(100)
let melon = prices.get("melon")      // None

let scores = [80, 90]
let outside = scores.get(-1)         // None; scores[-1] would fault
```

## E. Patterns

### E.1 List rest pattern

Uses: §1, §5, §6, §7, §9, §22

```topaz
function describe(values: Array<int>) -> string {
    match values {
        case [] => "empty"
        case [only] => "one: {only}"
        case [head, ..tail] => "head: {head}, rest: {tail.length}"
    }
}
```

### E.2 Empty list pattern

Uses: §3, §5, §6, §7, §9

```topaz
function isEmpty(items: Array<int>) -> bool {
    match items {
        case [] => true
        case _ => false
    }
}
```

### E.3 Nested record patterns

Uses: §3, §5, §6, §7

```topaz
type Point = { x: int, y: int }

function classify(p: Point) -> string {
    match p {
        case { x: 0, y: 0 } => "origin"
        case { x: 0 } => "on y-axis"
        case { y: 0 } => "on x-axis"
        case _ => "general"
    }
}
```

## F. Pipelines and Lambdas

### F.1 Pipeline with named arguments and call insertion (Snippet)

Uses: §5, §7, §8, §11, §22

Assumes: `rawInput: Array<{ isValid: bool }>`,
`normalize: (Array<{ isValid: bool }>, standard: string) -> Array<{ isValid: bool }>`

```topaz
let cleaned = rawInput
    |> normalize(standard: "UTF-8")
    |> filter(x => x.isValid)
```

Notes: both stages use first-positional insertion (§11); `standard:` is
a named argument following the inserted positional (§5).

### F.2 Placeholder `_` in pipeline (Snippet)

Uses: §10, §11, §22

Assumes: `fibonacci: (int) -> int`

```topaz
let series = 0..<10 |> map(_, fibonacci)
```

### F.3 Property sugar `|> .prop` (Snippet)

Uses: §8, §11

Assumes: `currentUser: { profile: { city: string } }`

```topaz
let cityName = currentUser
    |> .profile
    |> .city
```

Notes: `.field` sugar reads a record field from the piped value (§11).

### F.5 Closure capture

Uses: §2, §3, §7, §18

```topaz
function makeAdder(base: int) -> (int) -> int {
    return x => x + base
}

let add5 = makeAdder(5)
print("{add5(10)}")
```

Notes: the lambda captures `base` from the enclosing function scope (§18).

## G. Error Handling

### G.1 `Result` with `Ok` / `Err`

Uses: §1, §2, §3, §5, §7, §13, §22

```topaz
function divide(a: float, b: float) -> Result<float, string> {
    if b == 0.0 {
        return Err("Cannot divide by zero")
    }
    return Ok(a / b)
}
```

### G.2 `?` propagation with Option-returning conversion

Uses: §2, §3, §5, §6, §7, §8, §13, §22

```topaz
function parsePort(text: string) -> Result<int, string> {
    if text == "" {
        return Err("Port is required")
    }

    match toInt(text) {
        case Some(port) => Ok(port)
        case None => Err("Port must be an integer")
    }
}

function loadConfig(portText: string) -> Result<{ port: int }, string> {
    let port = parsePort(portText)?
    return Ok({ port: port })
}
```

### G.3 `defer` with file resource

Uses: §3, §5, §7, §13, §14, §22

```topaz
function writeLog(message: string) -> Result<(), string> {
    let file = open("app.log")?
    defer { file.close() }

    file.write(message)?
    return Ok(())
}
```

## H. Concurrency

### H.1 `concurrent` with timeout and `else` fallback (Snippet)

Uses: §3, §8, §9, §15, §22

Assumes: `userId: int`,
`loadUser: (int) -> Option<{ name: string }>`,
`loadPosts: (int) -> Array<string>`

```topaz
let dashboard = concurrent(timeout: 3s) {
    user: loadUser(userId)
    posts: loadPosts(userId)
} else {
    {
        user: None,
        posts: []
    }
}
```

Notes: the `else` record is type-compatible with the arm record (§15);
`None` and `[]` take contextual types from that rule (§22.1).

## I. String Templates

### I.1 Interpolation

Uses: §1, §4

```topaz
let name = "Topaz"
let greeting = "Hello, {name}!"
```

### I.2 `p"..."` path template

Uses: §1, §16

```topaz
let configPath = p"/etc/topaz/config.toml"
```

### I.3 `r"..."` regex template

Uses: §1, §16

```topaz
let identifierPattern = r"^[a-zA-Z_][a-zA-Z0-9_]*$"
```

Notes: Regex dialect details inside `r"..."`, including dot and escape
behavior beyond §16's reduced-escaping guarantee, are outside the v5.1
minimum. This pattern does not depend on dot or regex-escape behavior.

### I.4 `sh"..."` shell template

Uses: §1, §16

```topaz
let listCmd = sh"ls -la"
```

### I.5 `sql"..."` SQL template (Snippet)

Uses: §1, §16

Assumes: `userId: int`

```topaz
let query = sql"SELECT name FROM users WHERE id = {userId}"
```

Notes: interpolation produces a query parameter, not direct text insertion.

### I.6 Multiline string

Uses: §1

```topaz
let banner = """
    Topaz v5.1
    code becomes poetry
    """
```

### I.7 `sql"""..."""` multiline template

Uses: §1, §16

```topaz
let region = "EMEA"

let report = sql"""
    SELECT name, total
    FROM orders
    WHERE region = {region}
    ORDER BY total DESC
    """
```

Notes: interpolation produces a bound query parameter (§16); the
closing-delimiter whitespace prefix is stripped from each line (§1).

## J. Multilingual Identifiers

### J.1 Korean identifiers

Uses: §1, §5, §7

```topaz
function 인사하기(이름: string) -> string {
    return "안녕하세요, {이름}님!"
}

print(인사하기("토파즈"))
```

### J.2 Mixed-language and emoji identifiers

Uses: §1, §4, §7

```topaz
function привет(имя: string) -> string {
    return "Привет, {имя}!"
}

let 🚀rate = 0.15
```

## K. Optional and Advanced

### K.1 Optional chaining

Uses: §3, §8, §12, §22

```topaz
let maybeUser: Option<{ name: string }> = Some({ name: "Alice" })
let displayName = maybeUser?.name ?? "guest"
```

### K.2 Null coalescing assignment

Uses: §3, §4, §12

```topaz
let mut configPath: string | null = null
configPath ??= "default.toml"
```

### K.3 Default parameters with v5.1 restrictions

Uses: §1, §5, §7

```topaz
function greet(name: string, salutation: string = "Hello") -> string {
    return "{salutation}, {name}!"
}

print(greet("Topaz"))
print(greet("Topaz", "안녕"))
```

Notes: default values are literals or const expressions only; defaults do not
reference other parameters.

### K.4 Range with `by` step

Uses: §5, §10

```topaz
let evens = for x in 0..10 by 2 { x }

for i in 10..0 by -1 {
    print("{i}")
}
```

### K.5 Null-coalescing assignment on `Option`

Uses: §3, §4, §12, §22.1

```topaz
let mut cached: Option<int> = None
cached ??= Some(42)
```

Notes: no implicit `Some` wrapping occurs; `cached ??= 42` would be a
static error (§12).

## L. Generics

### L.1 Generic function with inference

Uses: §3, §5, §6, §7, §9, §22.1

```topaz
function head<T>(xs: Array<T>) -> Option<T> {
    match xs {
        case [] => None
        case [first, ..] => Some(first)
    }
}

let first: Option<int> = head([1, 2, 3])
```

### L.2 Generic type alias

Uses: §3, §8

```topaz
type Pair<T> = { first: T, second: T }

let bounds: Pair<int> = { first: 0, second: 100 }
```

### L.3 Variadic function type

Uses: §3, §5, §7, §10, §22

```topaz
function logAll(...items: string) -> () {
    for item in items {
        print(item)
    }
}

let sink: (...string) -> () = logAll
```

## M. Spread

### M.1 Call spread

Uses: §5, §7, §9, §10, §22

```topaz
function logWithPrefix(prefix: string, ...items: string) -> () {
    for item in items {
        print("{prefix}: {item}")
    }
}

let lines = Array.of("a", "b")
logWithPrefix("debug", ...lines)
logWithPrefix("debug", ...lines, "tail")
```

### M.2 Array spread

Uses: §9, §22

```topaz
let xs = Array.of(2, 3)
let ys = [1, ...xs, 4]
```

## N. Loops

### N.1 `while` and `break`

Uses: §1, §2, §4, §5, §22

```topaz
let mut total = 0
let mut n = 1

while true {
    if total + n > 100 {
        break
    }

    total = total + n
    n = n + 1
}

print("{total}")
```

## S. Spec-Only Forms

### S.1 Function composition `>>`

Uses: §2, §7, §11

```topaz
function double(x: int) -> int { x * 2 }
function increment(x: int) -> int { x + 1 }

let pipeline = double >> increment
let result = pipeline(5)
```

Notes: `>>` is defined at SPEC level (§2, §11) but is not part of any
current profile surface; public examples use `|>`.

## Out-of-Scope Examples

Per `SPEC.md` §20, these forms are forbidden or deferred in canonical v5.1
examples and must not appear here except as explicitly marked negative
examples:

- `mut let`.
- `[T]` as a public collection type.
- `function(T) -> U` in type positions.
- Legacy variadic form `args: ...T` (use `...args: T`).
- `[head, ...tail]` and `...rest` list patterns; pattern rest is `..`.
- JavaScript-style record spread `{ ...value }`.
- Fixed-arity tuple spread and iterable spread; v5.1 spread is Array-only and
  variadic-tail-only in calls.
- Receiver syntax `this`, `&self`, `&mut self`, implicit receiver declarations,
  and native method blocks.
- Rust interop tokens in canonical Topaz examples.
- `++`, `--`, and `**=`.
- Backtick tagged templates and `${expr}` interpolation.
- `html"..."` and HTML tagged templates.
- User-defined template tags; the v5.1 registry is `p`, `r`, `sh`, `sql`.
- `assert` as a keyword or statement form; the test-profile `assert(...)`
  function is not part of canonical public examples.
- `try` keyword expressions or statements. Postfix Result propagation `expr?`
  remains canonical.
- `import` / `export` / `use` module syntax.
- A sole `_` as a binding identifier.
- String indexing `s[i]`, string `.length`, string slicing syntax, and grapheme
  APIs as core semantics.
- Explicit generic call-site type arguments such as `f<int>(x)`; generic bounds,
  interfaces, user-implementable protocols, and variance annotations.
- Recursive alias cycles such as `type Node = { next: Node }`.
- Labeled `break` and `break value`.
- Optional-property pipe sugar `|> ?.field`.
- `x in map` (use `x in map.keys`) and `Map`/`Set` equality.
- Anonymous `function(...) -> T { ... }` expressions; use lambdas.
- `async` / `await`, automatic-async claims, catchable faults, panic keywords,
  and throw/catch.
- `concurrent { ... } else { ... }` without a timeout, and
  `concurrent(timeout: d) { ... }` without `else`.
