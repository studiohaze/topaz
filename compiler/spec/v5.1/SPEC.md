# Topaz Language v5.1 Specification

Status: v5.1 locked — structural, semantic, and feature passes (Steps 2–4)
landed.

This document is the single source of truth for canonical Topaz v5.1 language
syntax and semantics. Worked examples belong in `EXAMPLES.md`; usage policy
belongs in `PROFILES.md`; decision and conflict history belongs in
`DECISIONS.md`.

## §0 Authority and Scope

### Grammar

N/A.

### Semantics

Topaz v5.1 is expression-oriented, Result-first, Unicode-friendly, and uses
English keywords with local-language identifiers.

Normative authority order after v5.1 publication:

1. `topaz-v5.1/SPEC.md`.
2. `topaz-v5.1/PROFILES.md`, for surface-specific restrictions.
3. topaz.ooo canonical documentation.
4. Downstream consumers such as CSKernel pages.

`SPEC_V4.md`, `TOPAZ_AGENT_PACK.md`, the v5.0 documents under `topaz-v5/`, and
current public pages are legacy inputs. They do not override this document.

### Constraints

This file defines language forms, not marketing copy and not worked examples.
Any public example using a form absent from this file is non-canonical unless a
later v5.1 decision adds that form.

### Lowering / IR Notes

Lowering notes in this document are informative. They describe intended
implementation strategy but do not override grammar, typing, or semantic rules.

## §1 Lexical Structure

### Grammar

```ebnf
Identifier      ::= IdentifierStart IdentifierContinue*
IdentifierStart ::= UnicodeLetter | "_" | Emoji
IdentifierContinue ::= IdentifierStart | UnicodeNumber
```

```text
Constraint: an Identifier must not consist solely of the single
character "_".
```

```ebnf
IntegerLiteral  ::= DecimalInteger
FloatLiteral    ::= DecimalInteger "." DecimalDigits
StringLiteral          ::= SingleLineString | MultiLineString
SingleLineString       ::= '"' SingleLineStringPart* '"'
SingleLineStringPart   ::= EscapedChar | Interpolation | NonQuoteNonNewlineChar
MultiLineString        ::= '"""' MultiLineStringPart* '"""'
MultiLineStringPart    ::= EscapedChar | Interpolation | MultiLineRawChar
EscapedChar     ::= "\\" ("n" | "t" | "r" | "\\" | "\"" | "{" | "}")
Interpolation   ::= "{" Expression "}"

BoolLiteral     ::= "true" | "false"
NullLiteral     ::= "null"
UnitLiteral     ::= "(" ")"
Literal         ::= IntegerLiteral | FloatLiteral | StringLiteral
                 | BoolLiteral | NullLiteral | UnitLiteral

LineComment     ::= "//" chars-until-line-end
BlockComment    ::= "/*" chars-until-first-"*/" "*/"
```

`NonQuoteNonNewlineChar` is any source character other than `"`, `\`, `{`, `}`,
or a line terminator, except as part of `EscapedChar` or `Interpolation`.
`MultiLineRawChar` is any source character sequence that is not an unescaped
`"""` delimiter and not the start of `EscapedChar` or `Interpolation`.

Keywords are reserved and cannot be identifiers: `let`, `mut`, `const`,
`type`, `function`, `return`, `if`, `else`, `match`, `case`, `for`, `in`,
`by`, `while`, `break`, `continue`, `defer`, `concurrent`, `true`, `false`,
and `null`.

(Note: `while`, `break`, `continue` are the only v5.1 additions, per the
locked keyword budget. Duration units and template tags remain registry
entries, not keywords.)

### Semantics

Identifiers may use Korean, English, Russian, other Unicode letter scripts,
underscore, and emoji. Keyword spelling remains English.

v5.1 integer and float literals are decimal. Alternate bases and numeric
underscore separators are not part of v5.1 canonical syntax.

String interpolation evaluates the expression inside `{...}` at runtime and
converts the result to string using the standard string conversion operation.

A triple-quoted string may contain newlines and unescaped single `"` characters;
its delimiter is the first unescaped `"""`, so including the sequence `"""`
requires escaping at least one quote. Escapes and `{expr}` interpolation are
identical to single-line strings.

If the opening `"""` is immediately followed by a line terminator, that first
line terminator is not part of the content; otherwise content begins
immediately after the opening delimiter. The closing delimiter and any
whitespace that precedes it on the closing-delimiter line are not part of the
content.

Indentation stripping is based on the exact whitespace prefix before the
closing delimiter. If the closing delimiter is preceded on its line only by
spaces and tabs, that exact prefix is the closing indent; otherwise the closing
indent is empty and no indentation stripping occurs. For each non-blank content
line, the line must begin with the closing indent; otherwise the literal has a
static indentation error. When the check succeeds, that exact prefix is removed
from each non-blank content line. Tabs are matched as literal tab characters;
there is no visual-column expansion in the language definition.

`()` is the unit literal. It is the sole value of the unit type `()`
(§3). In expression position it is distinguished from a parenthesized
expression by the absence of any enclosed expression.

The single character `_` is a reserved discard/placeholder token. Its
roles are defined where they occur: wildcard pattern (§6) and pipeline
placeholder (§11). Identifiers that begin with an underscore and
contain at least one further character (such as `_internal`) are
ordinary identifiers.

Strings are immutable Unicode text values. String equality compares
Unicode scalar value sequences; no implicit normalization is
performed, so canonically equivalent but differently normalized
strings are not automatically equal. Storage encoding is informative,
not semantic; UTF-8 is recommended but not required for conformance.

Token recognition uses longest match. In particular, `?.` is a single
optional-access token: `expr?.field` is optional access (§12), never
postfix Result propagation (§13) followed by member access. To apply
member access to a propagated result, parenthesize: `(expr?).field`.

The three-character token `...` is lexed by longest match and is
distinct from `..` and `..<`. `...` appears only in spread positions
(§5, §9) and variadic forms (§3, §7); pattern rest remains the
two-character `..` (§6).

A tagged template is recognized lexically when an identifier-like tag
is immediately followed by a string delimiter with no intervening
whitespace (registry validation in §16). With whitespace, the
identifier and the string literal are separate tokens, and an
identifier followed by a string literal is never a call; `p "x"` is a
parse error.

### Constraints

Single-quoted strings and character literals are not canonical v5.1 Topaz.
Empty character literals such as `''` are forbidden. Single-line string
literals do not contain unescaped newlines; multiline content requires the
triple-quoted form.

Block comments do not nest.

`_` is not a bindable identifier and cannot be referenced as a value.
`let _ = expr` is permitted: the left-hand side is the wildcard
pattern (§6) and discards the value.

Strings are not indexable in v5.1. Applying index syntax to a value of type
`string`, such as `s[i]`, is a static error. Strings expose no `.length`
property in v5.1; `s.length` is a static error when `s` has type `string`.
Scalar-level access is provided by the standard library (§22). Direct string
slicing syntax and grapheme-cluster APIs are deferred (§20).

### Lowering / IR Notes

Interpolated strings lower to string concatenation or a runtime formatting node
with source-span metadata.

## §1a Layout and Statement Separation

### Grammar

A semicolon `;` is an explicit separator in separator mode. It separates the
current item in a `Program`, `BlockExpr`, `match` body, or `concurrent` body.
It is not a delimiter-list separator in continuation mode; parameter,
argument, element, field, type, and pattern lists use commas. The
newline-significance rules below define the implicit separator.

Trailing-continuation tokens:

```text
unary operators:          + - ! ~
binary operators:         ** * / % + - .. ..< < <= > >= == != in && || ?? >> |>
assignment operators:     = += -= *= /= %= ??=
other continuation forms: , => . ?. by
opening delimiters:       ( [
```

Leading-continuation tokens:

```text
postfix/member forms:     . ?.
non-prefix binary ops:    ** * / % .. ..< < <= > >= == != in && || ?? >> |>
assignment operators:     = += -= *= /= %= ??=
range-step keyword:       by
control continuation:     else
```

### Semantics

A significant token is any token other than whitespace or comments.

Every delimiter push establishes a layout mode:

- **Separator mode** — a newline may separate complete items. Separator
  mode is used for: the `Program` top level (item: `TopLevelItem`);
  `BlockExpr` statement lists (item: `Statement` or final expression);
  `match` bodies (item: case clause); `concurrent` bodies (item:
  concurrent arm).
- **Continuation mode** — a newline is insignificant for item
  separation. Continuation mode is used for every delimiter context
  that contains an expression, type, pattern, parameter, argument,
  element, or field list rather than a statement/item list, including:
  parenthesized expressions and call argument lists; function parameter
  lists and function-type parameter lists; array literals, array
  constants, array patterns, and index expressions; record literals,
  record constants, record updates, record types, and record patterns;
  template interpolations.

A physical newline is a potential separator only in separator mode. It
ends the current item unless one of the following holds:

1. the parser is in continuation mode;
2. the previous significant token is a trailing-continuation token;
3. the next significant token is a leading-continuation token.

In a `BlockExpr`, an expression item followed by an explicit semicolon or by a
significant newline separator is an `ExprStmt`. An expression item followed
only by ignored empty separators before the closing `}` is the optional final
expression and determines the block value. Non-expression statements are always
statements.

Empty separators are ignored: a blank line, a newline immediately after
an opening separator delimiter, or a newline immediately before a
closing delimiter does not create an empty statement, case clause, or
arm.

### Constraints

`{` is intentionally in neither continuation token set. Brace behavior
is controlled entirely by mode classification: record
literal/update/type/pattern braces are continuation mode; block, match
body, and concurrent body braces are separator mode.

`+` and `-` are trailing-continuation only; they are not
leading-continuation tokens because they can begin unary expressions.
Leading `+`/`-` arithmetic style requires explicit delimiters or a
trailing operator on the previous line.

A `}` followed by a newline and then `else` continues the same `if` or
`concurrent(timeout: ...)` form; `else` is a leading-continuation token.

Canonical examples omit semicolons.

### Lowering / IR Notes

Layout is resolved entirely at parse time and has no lowering footprint.

## §2 Operators and Precedence

### Grammar

```ebnf
OperatorExpr    ::= UnaryExpr | BinaryExpr | ComposeExpr | PipeExpr
UnaryExpr       ::= UnaryOp Expression
BinaryExpr      ::= Expression BinaryOp Expression
UnaryOp         ::= "+" | "-" | "!" | "~"
BinaryOp        ::= "**" | "*" | "/" | "%" | "+" | "-"
                 | ".." | "..<"
                 | "<" | "<=" | ">" | ">=" | "==" | "!=" | "in"
                 | "&&" | "||" | "??"
```

`ComposeExpr` and `PipeExpr` are defined in §11 and are reachable here as
`OperatorExpr` alternatives. `|>` is the only operator whose right-hand
side uses `PipeRhs` rather than `Expression`; the precedence table keeps
levels 11 and 12 for `>>` and `|>`.

Operators are listed from highest precedence to lowest:

| Level | Operators                                                                                               | Associativity |
| ----- | ------------------------------------------------------------------------------------------------------- | ------------- |
| 1     | calls, indexing, member access, optional access, result propagation: `()`, `[]`, `.`, `?.`, postfix `?` | left          |
| 2     | exponentiation: `**`                                                                                    | right         |
| 3     | unary: `+`, `-`, `!`, `~`                                                                               | right         |
| 4     | multiplicative: `*`, `/`, `%`                                                                           | left          |
| 5     | additive: `+`, `-`                                                                                      | left          |
| 6     | range: `..`, `..<`                                                                                      | left          |
| 7     | comparison and membership: `<`, `<=`, `>`, `>=`, `==`, `!=`, `in`                                       | left          |
| 8     | logical and: `&&`                                                                                       | left          |
| 9     | logical or: `\|\|`                                                                                      | left          |
| 10    | null coalescing: `??`                                                                                   | left          |
| 11    | function composition: `>>`                                                                              | right         |
| 12    | pipeline: `\|>`                                                                                         | left          |

### Semantics

`int` is exactly 64-bit signed two's complement, with range
-9223372036854775808 through 9223372036854775807. Integer `/`
truncates toward zero. Integer `%` satisfies
`a == (a / b) * b + (a % b)` for nonzero `b` and takes the sign of
`a` or is zero. Integer `/` or `%` by zero faults (§13a). Runtime
integer operations that overflow fault (§13a); integer operations in
`ConstExpression` that overflow or divide by zero are static errors.

`float` is IEEE-754 binary64. `x / 0.0` yields signed infinity;
`0.0 / 0.0` yields NaN; `NaN != NaN`.

Except for string concatenation with `+`, binary arithmetic operators require
operands from the same numeric domain in v5.1; there is no implicit `int` ↔
`float` widening or narrowing. `+`, `-`, `*`, `/`, and `**` are defined for
same-domain `int` operands and same-domain `float` operands. `%` is defined
only for `int` operands in v5.1; floating-point remainder is a standard-library
matter.

`int ** int` returns `int`. Its exponent must be non-negative. A negative
integer exponent in a `ConstExpression` is a static error; a dynamically
negative integer exponent faults (§13a). Integer exponentiation overflow faults
at runtime and is a static error in `ConstExpression`. Use `float` operands for
negative exponents.

`+` also performs string concatenation when both operands are
strings. `&&` and `||` short-circuit left to right.

`==` and `!=` are defined only for **comparable types**. Comparable in
v5.1: `int`, `float`, `string`, `bool`, `null`, and `()`; and literal
types, by their underlying literal value. Float equality follows
IEEE-754, so `NaN != NaN`.

A union type is comparable iff every member type is comparable. Equality on a
union value compares the actual runtime value using the equality relation of
its member type. This includes nullable unions such as `T | null` when `T` is
comparable.

Records whose fields are all comparable are comparable, compared
field-wise by field name independent of source order. Record values are
comparable only at compatible record shapes; comparing records with
incompatible field sets is a static type error. Arrays whose elements
are comparable are comparable, by length and element order. `Option<T>`
is comparable when `T` is comparable, and `Result<T, E>` when both `T`
and `E` are comparable.

Not comparable in v5.1: function values (comparison is a static
error), `File` and other resource handles, template values, and
`Map` / `Set` values. `Map` keys and `Set` elements must nevertheless
be comparable types (§9).

Structural comparison always terminates: comparable values are finite
trees by construction, since recursive type aliases are not v5.1
canonical (§20).

Assignment forms are statements, not expressions; their grammar lives
under `Statement` (§5). They do not participate in expression
precedence. `??=` is statement-only and is specified in §12.

### Constraints

`++` and `--` are not v5.1 Topaz. `**=` is not v5.1 canonical syntax; use an
explicit assignment form if exponent assignment is needed.

`by` is not a general operator. It appears only in range syntax (§10).

### Lowering / IR Notes

Compound assignments lower to a read, operation, and write of the assignment
target. Implementations must evaluate the target reference once.

## §3 Types

### Grammar

```ebnf
TypeDecl       ::= "type" Identifier TypeParams? "=" Type
TypeParams     ::= "<" Identifier ("," Identifier)* ">"
Type           ::= UnionType
UnionType      ::= PrimaryType ("|" PrimaryType)*
PrimaryType    ::= NamedType
                 | LiteralType
                 | RecordType
                 | FunctionType
                 | UnitType
                 | "(" Type ")"
NamedType      ::= Identifier TypeArgs?
TypeArgs       ::= "<" Type ("," Type)* ">"
LiteralType    ::= StringLiteral | IntegerLiteral | FloatLiteral
                 | BoolLiteral | NullLiteral
RecordType     ::= "{" FieldType ("," FieldType)* ","? "}"
FieldType      ::= Identifier ":" Type
FunctionType   ::= "(" FunctionTypeParams? ")" "->" Type
FunctionTypeParams ::= FunctionTypeParam ("," FunctionTypeParam)* ","?
FunctionTypeParam  ::= Type | "..." Type
UnitType       ::= "(" ")"
```

Primitive type names: `int`, `float`, `string`, `bool`, and `()`.

Standard generic type constructors: `Array<T>`, `Map<K, V>`, `Set<T>`,
`Option<T>`, and `Result<T, E>`.

### Semantics

`type` declares a type alias. Aliases do not create a distinct nominal type in
v5.1; they name the aliased type expression.

Union types represent values that may be any member type. Nullable types are
ordinary unions containing `null`, such as `string | null`.

Literal types restrict a value to the exact literal value. Literal union aliases
are canonical v5.1 syntax.

Function type positions use `(T) -> U`. This is the only canonical public
function type form.

Generic type aliases bind type parameters scoped to the alias body
and are erased after type checking, like all aliases. A variadic
function-type parameter `...T` describes the type of a function whose
final parameter is variadic with element type `T`; `(...T) -> U` and
`(string, ...string) -> ()` are canonical.

### Constraints

`[T]` is not a canonical public collection type. Use `Array<T>`.

`function(T) -> U` is not a canonical type-position form.

A variadic function-type parameter must be final; `(...T, U) -> V` is
a static error. Type parameters take no bounds, constraints, or
variance annotations in v5.1 (§20). Recursive type aliases — an alias
whose body mentions the alias being declared — are not canonical
v5.1 (§20).

Type parameter names in a `TypeParams` list must be unique. A type parameter
binds only in type positions; it is not a value binding and cannot be referenced
as a value. Within a generic type alias, type parameters are scoped to the alias
body and shadow outer type names only in that type scope.

Recursive alias cycles are static errors in v5.1: an alias body must not mention
the alias being declared, directly or through a cycle of aliases. Recursive and
inductive type aliases are deferred (§20).

### Lowering / IR Notes

Type aliases may be erased after type checking. Union and literal types lower to
type metadata or checks as required by the target runtime.

## §4 Bindings

### Grammar

```ebnf
Binding         ::= LetBinding | MutableBinding | ConstBinding
LetBinding      ::= "let" Pattern TypeAnnotation? "=" Expression
MutableBinding  ::= "let" "mut" Identifier TypeAnnotation? "=" Expression
ConstBinding    ::= "const" Identifier TypeAnnotation? "=" ConstExpression
TypeAnnotation  ::= ":" Type
```

### Semantics

`let` creates an immutable lexical binding. `let mut` creates a mutable lexical
binding. `const` creates a compile-time constant binding whose initializer must
be a const expression.

Topaz uses lexical block scoping. Inner blocks may shadow outer bindings.
Redeclaring the same name in the same scope is not canonical v5.1.

### Constraints

`mut let` is forbidden. Only `let mut` is canonical.

Mutable assignment requires a binding declared with `let mut` or an assignable
mutable member target. Immutable `let` and `const` bindings cannot be assigned.

### Lowering / IR Notes

Immutable bindings may lower to SSA values. Mutable bindings lower to mutable
storage cells or equivalent target-language representation.

## §5 Expressions

### Grammar

```ebnf
Program         ::= TopLevelItem*
TopLevelItem    ::= Declaration | NonDeclarationStatement
Declaration     ::= FunctionDecl | TypeDecl

Statement       ::= Declaration | NonDeclarationStatement
NonDeclarationStatement
                 ::= Binding
                  | Assignment
                  | ReturnStmt
                  | DeferStmt
                  | WhileStmt
                  | BreakStmt
                  | ContinueStmt
                  | ExprStmt

WhileStmt       ::= "while" Expression BlockExpr
BreakStmt       ::= "break"
ContinueStmt    ::= "continue"
```

```ebnf
Expression      ::= Literal
                 | Identifier
                 | BlockExpr
                 | IfExpr
                 | MatchExpr
                 | ForExpr
                 | ConcurrentExpr
                 | CallExpr
                 | MemberExpr
                 | IndexExpr
                 | OptionalAccess
                 | OptionalCall
                 | OperatorExpr
                 | LambdaExpr
                 | RecordLiteral
                 | RecordUpdate
                 | ArrayLiteral
                 | RangeExpr
                 | ResultValue
                 | TryExpr
                 | TaggedTemplate
BlockExpr       ::= "{" Statement* Expression? "}"
IfExpr          ::= "if" Expression BlockExpr ("else" (IfExpr | BlockExpr))?
MatchExpr       ::= "match" Expression "{" CaseClause+ "}"
CaseClause      ::= "case" Pattern Guard? "=>" Expression
Guard           ::= "if" Expression
ForExpr         ::= "for" Pattern "in" Expression BlockExpr
CallExpr        ::= Expression "(" CallArgs? ")"
CallArgs        ::= PositionalOrSpreadArg ("," PositionalOrSpreadArg)*
                    ("," NamedArg)* ","?
                 | NamedArg ("," NamedArg)* ","?
PositionalOrSpreadArg ::= Expression | "..." Expression
NamedArg        ::= Identifier ":" Expression
MemberExpr      ::= Expression "." Identifier
IndexExpr       ::= Expression "[" Expression "]"
ReturnStmt      ::= "return" Expression?
Assignment      ::= Assignable AssignmentOp Expression
AssignmentOp    ::= "=" | "+=" | "-=" | "*=" | "/=" | "%=" | "??="
Assignable      ::= Identifier | MemberExpr | IndexExpr
ExprStmt        ::= Expression
ConstExpression ::= Literal
                 | Identifier
                 | ArrayConst
                 | RecordConst
                 | "(" ConstExpression ")"
                 | UnaryConstOp ConstExpression
                 | ConstExpression BinaryConstOp ConstExpression
ArrayConst      ::= "[" (ConstExpression ("," ConstExpression)* ","?)? "]"
RecordConst     ::= "{" ConstField ("," ConstField)* ","? "}"
ConstField      ::= Identifier ":" ConstExpression
UnaryConstOp    ::= "+" | "-" | "!" | "~"
BinaryConstOp   ::= "+" | "-" | "*" | "/" | "%" | "**"
                 | "==" | "!=" | "<" | "<=" | ">" | ">="
                 | "&&" | "||" | "??"
```

### Semantics

Blocks evaluate statements in order. If the block ends with an expression, that
expression is the block value. If not, the block value is unit `()`.

`if` and `match` are expressions. `if` without `else` has type `()` unless used
only as a statement. `for` is an expression that collects each iteration body's
value into an `Array<T>` unless the body type is `()`, in which case it may be
used as a statement.

Function calls evaluate callee, then arguments left to right. Named arguments
bind by parameter name.

Positional arguments fill fixed parameters first, in order. A spread
argument `...expr` must have type `Array<T>` and may appear only at
or after the variadic tail position of the callee; multiple spread
and positional arguments may interleave within the tail, and every
tail contribution must be assignable to the variadic element type.
Named arguments bind by parameter name and must follow all positional
and spread arguments.

Spread arguments are positional-class arguments. For call checking, process the
positional/spread argument prefix from left to right against the callee's formal
parameters. A non-spread positional argument fills the next fixed positional
parameter until the variadic tail is reached; after the variadic tail is
reached, non-spread positional arguments contribute one element to the tail. A
spread argument is valid only when the next positional target is already the
variadic tail. Therefore a spread cannot skip an unsatisfied fixed parameter and
cannot be made valid by a later named argument.

Named arguments bind fixed parameters by name after the positional/spread prefix
has been processed. They do not contribute to a variadic tail. A missing
required fixed parameter, a duplicate argument binding, a named argument for a
nonexistent parameter, or a spread argument at a call whose callee has no
variadic tail is a static error.

`return` exits the current function immediately. A function may also return the
final expression of its body.

`while cond { ... }` evaluates `cond`, which must have type `bool`,
and executes the body while it is `true`. `while` is a statement and
produces no value; its body block's internal value, if any, is
discarded.

`break` terminates the innermost enclosing `for` or `while`;
`continue` skips to that loop's next iteration. Both are statements,
have no value, and target only the innermost enclosing loop.

Declarations may appear inside blocks. Nested `function` and `type`
declarations are lexically scoped to the enclosing block, follow §4
shadowing rules, and cannot redeclare another name in the same scope.

The normative standard-library surface used by canonical examples —
including `print`, `open`, `toInt`, the collection helpers, and the
reserved names `JSON.parse` and `regex.match` — is specified in §22.
These are ordinary library names, not special grammar.

### Constraints

All `match` cases must use `case`. A catch-all case uses `_`.

If a `match` is used as a value, its cases must have compatible result types.
Exhaustiveness checking is required when the scrutinee type is finite enough to
check statically; otherwise a catch-all case is required for canonical public
examples.

In expression position, `{` opens a record literal iff the next
significant tokens are `Identifier ":"`; otherwise it opens a
`BlockExpr`. `{}` is an empty block whose value is `()`; empty record
literal syntax does not exist (§8's `FieldInit` list is non-empty).
The same colon-first rule applies in `ConstExpression` position for
`RecordConst`. Brace contexts following `if`, `else`, `for`, `while`,
`function`, `match`, and `concurrent` are determined by those
constructs; type and pattern positions are determined by the
surrounding construct (`RecordType`, `RecordPattern`) and do not use
expression-position lookahead.

A `{` immediately following a complete expression without an explicit or
implicit item separator opens a `RecordUpdate` iff the first significant tokens
inside the braces are `Identifier ":"`; otherwise the `{` is not part of that
expression. Record-update braces use continuation mode. A block expression that
follows another expression as a new item must be separated by an explicit or
implicit separator.

A spread argument cannot satisfy a fixed positional parameter; there
is no fixed-arity tuple spread and no iterable spread in v5.1 (§20).
A named argument preceding a positional or spread argument is a
static error.

`break` and `continue` outside a `for` or `while` body — including at
top level — are static errors. A `for` used as a value-collecting
expression is a static error if a `break` or `continue` targets that
`for`; a nested loop's own `break`/`continue` does not affect the
outer `for`'s value-collecting status. There is no labeled `break`
and no `break value` (§20).

`return` is valid only inside the body of a `function` declaration or
a lambda, and exits the innermost enclosing one; top-level `return`
is a static error. These are semantic constraints; the closed §5
grammar is unchanged.

### Lowering / IR Notes

Expression blocks lower to scoped regions. `for` expressions lower to iteration
plus result collection when the body is non-unit.

## §6 Patterns

### Grammar

```ebnf
Pattern         ::= WildcardPattern
                 | LiteralPattern
                 | RangePattern
                 | BindingPattern
                 | TypePattern
                 | ConstructorPattern
                 | ListPattern
                 | RecordPattern

WildcardPattern ::= "_"
LiteralPattern  ::= Literal
RangePattern    ::= ConstExpression ".." ConstExpression
                 | ConstExpression "..<" ConstExpression
BindingPattern  ::= Identifier
TypePattern     ::= Identifier ":" Type
ConstructorPattern ::= Identifier "(" PatternList? ")"
PatternList     ::= Pattern ("," Pattern)* ","?
ListPattern     ::= "[" ListPatternElems? "]"
ListPatternElems ::= ListPatternRest
                 | Pattern ("," Pattern)* ("," ListPatternRest)? ","?
ListPatternRest ::= ".." Pattern?
RecordPattern   ::= "{" RecordPatternField ("," RecordPatternField)* ","? "}"
RecordPatternField ::= Identifier (":" Pattern)?
```

### Semantics

Patterns destructure values in `let`, `for`, `match`, and function-like binding
contexts where allowed by the grammar.

Type patterns bind the value if the value conforms to the type. Constructor
patterns match standard constructors such as `Some`, `None`, `Ok`, and `Err`.
Range patterns match values contained in the inclusive or upper-exclusive
range. Range-pattern endpoints are const expressions evaluated at compile
time.

List rest patterns use `..`. The rest binding, if present, receives the
unmatched slice as an `Array<T>`.

Record patterns match named fields. A field without an explicit subpattern binds
the field value to a binding with the same name.

### Constraints

The `..` rest marker in a list pattern must occur at an element
boundary: the start of the list pattern or immediately after a comma.
Consequently `[1..5]` is always a single range pattern and
`[1, ..rest]` is a list rest pattern. `...rest` is forbidden. Only
one rest marker may appear in a list pattern. Range patterns may
appear in `match` cases; they perform containment matching and cannot
introduce new names.

### Lowering / IR Notes

Range patterns lower to range containment checks. List patterns lower to length
checks, indexed loads, and slice creation. Record patterns lower to
field-existence checks and field loads.

## §7 Functions

### Grammar

```ebnf
FunctionDecl    ::= "function" Identifier TypeParams? ParameterList ReturnType? BlockExpr
ParameterList   ::= "(" Parameters? ")"
Parameters      ::= Parameter ("," Parameter)* ","?
Parameter       ::= VariadicParameter | NormalParameter
NormalParameter ::= Identifier ":" Type DefaultValue?
VariadicParameter ::= "..." Identifier ":" Type
DefaultValue    ::= "=" ConstExpression
ReturnType      ::= "->" Type

LambdaExpr      ::= LambdaParams "=>" Expression
LambdaParams    ::= Identifier | "(" LambdaParamList? ")"
LambdaParamList ::= LambdaParam ("," LambdaParam)* ","?
LambdaParam     ::= Identifier TypeAnnotation?
```

### Semantics

Function declarations bind a function name in the current scope. Parameters are
immutable bindings unless the function body creates mutable local copies.

Default parameter values are evaluated at call time when the argument is
omitted. v5.1 default values are restricted to literals and const expressions.
One default parameter may not reference another parameter.

Variadic parameters collect zero or more arguments into an `Array<T>`. A
variadic parameter must be the final parameter.

Lambdas create closures over lexical scope as defined in §18.

Generic function declarations provide rank-1 parametric polymorphism:
type parameters are bound at the declaration and instantiated at each
call site by inference. When local inference cannot solve a type
parameter, the expression requires a contextual type under the §22.1
contextual-typing rule; an unsolved type parameter without context is
a static error.

Type parameter names in a generic function declaration must be unique. Function
type parameters are scoped to the declaration's parameter types, return type,
and body. They bind only type positions; they are not value parameters and
cannot be referenced as values.

### Constraints

Canonical variadic syntax is `...args: T`. The legacy form `args: ...T` is
forbidden.

Type parameters are rank-1 only: a type parameter may not itself be
instantiated polymorphically, and lambda parameters always receive
concrete types by inference. There are no bounds, constraints, or
variance annotations (§20). Explicit call-site type arguments such as
`f<int>(x)` are not v5.1 syntax (§20); pin types with an annotated
binding, parameter, or return position instead.

Anonymous `function(...) -> T { ... }` expressions are not canonical v5.1; use
lambdas for anonymous functions.

### Lowering / IR Notes

Default parameters lower to call-site or function-entry omission checks.
Variadics lower to array construction for the variadic tail.

## §8 Records

### Grammar

```ebnf
RecordLiteral   ::= "{" FieldInit ("," FieldInit)* ","? "}"
FieldInit       ::= Identifier ":" Expression
FieldAccess     ::= Expression "." Identifier
RecordUpdate    ::= Expression "{" FieldUpdate ("," FieldUpdate)* ","? "}"
FieldUpdate     ::= Identifier ":" Expression
```

### Semantics

Records are structural values with named fields. Field access reads the named
field. Record update creates a shallow copy of the original record with the
listed fields replaced.

If a field appears multiple times in a record update, the last update wins.

### Constraints

JavaScript-style record spread is not v5.1 Topaz. Use record update syntax for
record changes.

Receiver declarations, implicit `this`, `&self`, and `&mut self` are not native
Topaz v5.1 record syntax.

Expression-position and postfix-position disambiguation among record literals,
record updates, and block expressions is defined in §5; record types and
record patterns are construct-determined and unaffected.

### Lowering / IR Notes

Record updates lower to copy-plus-set operations. Implementations may optimize
with persistent data structures or target-language record-copy primitives.

## §9 Collections

### Grammar

```ebnf
ArrayLiteral    ::= "[" (ArrayElement ("," ArrayElement)* ","?)? "]"
ArrayElement    ::= Expression | "..." Expression
IndexAccess     ::= Expression "[" Expression "]"
CollectionType  ::= "Array" "<" Type ">"
                 | "Map" "<" Type "," Type ">"
                 | "Set" "<" Type ">"
```

### Semantics

`Array<T>` is the canonical public sequential collection type. `Map<K, V>` is
the canonical key-value collection type. `Set<T>` is the canonical uniqueness
collection type.

Array literals produce `Array<T>` values when their elements have a compatible
type. Indexing reads an element by index.

An array-spread element `...expr` must have type `Array<T>` and
expands its elements in place. The containing literal's element type
is the common element type of the non-spread elements and the spread
arrays' element types.

The standard-library collection surface canonical for examples is
specified in §22: `Array.of`, `Map.new`, `Set.of`, `Array.push`,
`Map.insert`, `Set.add`, `map.keys`, and `arr.length` (arrays only;
strings expose no `.length`, §1).

Mutation and iteration order: removing an existing key or element
removes it from the iteration order; re-inserting a removed key or
element appends it at the end; updating an existing map key's value
does not change its key order.

### Constraints

Collection mutation requires a mutable binding when the mutation changes the
collection in place.

`[T]` is not a collection type form.

`Map<K, V>` is well-typed in canonical v5.1 only when `K` is a comparable type
(§2); using a non-comparable map key type is a static error. `Set<T>` is
well-typed in canonical v5.1 only when `T` is comparable; using a
non-comparable set element type is a static error.

Membership with `in` is defined for arrays and sets when the tested value and
the element type are compatible comparable types; for ranges under the range
membership rule (§10); and for `map.keys`, whose value is an `Array<K>` (§22).
There is no canonical `in` overload for `Map<K, V>` itself. `x in map` is a
static error; use `x in map.keys`.

Record spread remains forbidden (§20). Iterable spread is deferred;
spread is `Array`-only in v5.1.

### Lowering / IR Notes

Collection operations lower to target standard-library calls or runtime helper
calls. `k in map.keys` may lower to a map key-membership primitive.

## §10 Ranges

### Grammar

```ebnf
RangeExpr       ::= InclusiveRange | UpperExclusiveRange
InclusiveRange  ::= Expression ".." Expression ("by" Expression)?
UpperExclusiveRange ::= Expression "..<" Expression ("by" Expression)?
```

### Semantics

`a..b` is inclusive at both ends. `a..<b` excludes the upper bound. `by step`
sets the stride. A negative step iterates backward.

Range element types must support ordered stepping. v5.1 canonical examples may
use integer ranges. Other step-capable types require standard-library support.

When `by` is omitted, the range step is `1`.

Range emptiness and stepping: `a..b by step` with a positive step is
empty when `a > b`; with a negative step it is empty when `a < b`.
An inclusive range includes its upper endpoint only when the endpoint
lands exactly on the step sequence; `..<` excludes the upper bound
under the same stepping rule.

Iteration protocol: the `for`-eligible types in v5.1 are `Array<T>`
(increasing index order), integer ranges (stride order), `Set<T>`
(insertion order), and `map.keys` (key insertion order, as
`Array<K>`). Strings are not directly `for`-iterable in v5.1; use the
string standard-library surface (§22). `Iterable<T>` is a
prelude-only protocol name used by standard-library signatures (§22);
it is not user-implementable, and user-defined iterable protocols are
deferred (§20).

Range membership with `in` uses the same stepped sequence as iteration: a value
is in a range iff it would be yielded by iterating that range with its effective
step. Endpoint inclusivity and exclusivity are therefore interpreted after the
step rule is applied.

### Constraints

The §2 precedence row for `..`/`..<` governs range parsing; `by` is
valid only inside a `RangeExpr` and is not a general operator. The
step must not be zero: a dynamic step of zero faults (§13a); a
constant step of zero is a static error.

### Lowering / IR Notes

Ranges lower to range objects or loop bounds plus stride checks.

## §11 Pipelines and Placeholders

### Grammar

```ebnf
PipeExpr        ::= Expression "|>" PipeRhs
PipeRhs         ::= Expression | "." Identifier
Placeholder     ::= "_"
ComposeExpr     ::= Expression ">>" Expression
```

### Semantics

For `lhs |> rhs`, the left-hand side is evaluated exactly once before any
right-hand side subexpression is evaluated, and the resulting value is saved as
a temporary for lowering. The right-hand side is parsed normally under the
expression grammar, except for the `.field` pipe sugar alternative.

If `rhs` contains one or more pipeline placeholders, placeholder replacement has
priority and no implicit argument insertion also occurs. A pipeline placeholder
is a `_` token in expression position inside the argument list of a `CallExpr`
or `OptionalCall`. `_` tokens in patterns are wildcard patterns (§6), not
pipeline placeholders. A `_` token used as a callee, member name, field label,
standalone RHS, or any other non-argument expression position is a static
error. Every valid pipeline placeholder in the RHS is replaced by the same
saved left-hand value; ordinary call/optional-call evaluation then proceeds
under §5/§12.

If there are no placeholders, `.field` lowers to member access on the saved
left-hand value; otherwise, if `rhs` is a call expression, the saved left-hand
value is inserted as the first positional argument before all explicit
positional, spread, and named arguments; otherwise, if `rhs` has callable type,
the pipeline lowers to a call of `rhs` with the saved left-hand value as its
single argument; otherwise the pipeline expression is a static error.

`f >> g` composes functions right-associatively with typing
`((A) -> B) >> ((B) -> C) : (A) -> C`; there is no multi-argument
composition in v5.1.

### Constraints

`_` binds to the nearest call expression that contains it; a bare `_`
outside placeholder contexts is not canonical (§1). Optional-property
pipe sugar `|> ?.field` is not v5.1 syntax (§20); ordinary postfix
optional chaining covers the use case. A pipeline right-hand side
whose named arguments precede positional arguments is invalid before
lowering (§5).

### Lowering / IR Notes

Pipelines and composition lower to lambda or direct-call forms after placeholder
binding is resolved.

## §12 Optional Chaining and Null

### Grammar

```ebnf
OptionalAccess  ::= Expression "?." Identifier
OptionalCall    ::= Expression "?." Identifier "(" CallArgs? ")"
NullCoalesce    ::= Expression "??" Expression
NullAssign      ::= Assignable "??=" Expression
```

### Semantics

There are two optional containers with separate canonical roles:
`Option<T>`, the canonical Topaz absence model, and nullable unions
`T | null`, the data-boundary and interop shape. The canonical
standard library returns `Option`, not `T | null`, except for APIs
whose purpose is to model external nullable data.

`??` is typed by the static type of its left operand and unwraps
exactly one layer: if `a: Option<T>` and `b: T`, then `a ?? b : T`,
where `Some(v) ?? b` evaluates to `v` and `None ?? b` evaluates to
`b`; if `a: T | null` and `b: T`, then `a ?? b : T`, where a non-null
`a` evaluates to its value and `null ?? b` evaluates to `b`.

`??` evaluates its left operand exactly once. The right operand is evaluated
only when the left operand is `None` or `null`; otherwise it is not evaluated.

`?.` is valid only when the left-hand side is `Option<T>` or a
nullable union, unwraps exactly one layer, and preserves the
container kind: if `a: Option<T>` then `a?.field : Option<U>` and
`a?.method(args) : Option<U>`; if `a: T | null` then
`a?.field : U | null` and `a?.method(args) : U | null`. Chained
optional access preserves the current container model; it does not
collapse `Option` into `null`.

For `?.`, the receiver expression is evaluated exactly once. If the receiver is
`None` or `null`, the access short-circuits and produces the corresponding empty
container (`None` or `null`) without evaluating method-call arguments. If the
receiver is present, field access or method-call evaluation proceeds normally;
method-call arguments are then evaluated left to right.

`target ??= value` is statement-only and is valid only when the
target type is `Option<T>` or a nullable union containing `null`. If
`target: T | null`, `value` must be assignable to `T | null`; if
`target: Option<T>`, `value` must be assignable to `Option<T>` — no
implicit `Some(value)` wrapping occurs. Semantics: if `target` is
`null` or `None`, assign `value`; otherwise leave `target` unchanged.
The assignment target is evaluated exactly once.

### Constraints

`?.` is not a general safe-navigation operator for ordinary non-null
records; use `.` when the value is not optional or nullable. `??=` on
a non-optional, non-nullable target is a static error. Nested
optional shapes such as `Option<Option<T>>` and `Option<T | null>`
are well-defined by the same one-layer rules but non-canonical in
public examples. An expression whose union includes both `Option<T>`
and `null` is non-canonical unless explicitly matched first.

### Lowering / IR Notes

Optional chaining lowers to `Option.map` / `Option.flatMap` or equivalent
nullable checks. `??=` lowers to an if-null-then-assign statement.

## §13 Result and Error Handling

### Grammar

```ebnf
ResultType      ::= "Result" "<" Type "," Type ">"
ResultValue     ::= "Ok" "(" Expression ")" | "Err" "(" Expression ")"
TryExpr         ::= Expression "?"
```

### Semantics

Topaz v5.1 canonical error handling is Result-first. `Result<T, E>` contains
either `Ok(T)` or `Err(E)`.

`expr?` requires `expr` to have `Result<T, E>` type. If the value is `Ok(v)`,
the expression evaluates to `v`. If the value is `Err(e)`, the current function
returns `Err(e)` immediately.

### Constraints

`?` may only appear in a function or closure whose return type can carry the
same error value. Nested accidental errors such as `Err(Err(...))` are
non-canonical.

`try` expressions and `assert` statements are not v5.1 canonical syntax.

There is no panic keyword in v5.1 (runtime faults are specified in
§13a). Public examples should model recoverable failure with `Result`.

### Lowering / IR Notes

`?` lowers to a branch on the result constructor and early return on `Err`.

## §13a Runtime Faults

### Grammar

N/A.

### Semantics

A **fault** aborts the current program evaluation. A fault is not a
value: it is not `Result`, not `Option`, and cannot be caught by `?`
(§13), `??` (§12), or `concurrent` (§15).

Fault sources in v5.1:

1. direct array indexing out of bounds or with a negative index (§9);
2. integer `/` or `%` by zero (§2);
3. a dynamic range step of zero (§10);
4. integer overflow in runtime integer arithmetic (§2);
5. integer exponentiation with a dynamically negative exponent (§2);
6. a runtime `match` miss, where exhaustiveness was not statically
   provable and no catch-all case exists (§5);
7. test-profile `assert` failure, only when the test profile is active
   (§22; PROFILES);
8. any other source explicitly listed by a future ADR.

Static equivalents are static errors, not faults: constant division by
zero, constant integer overflow, constant negative integer exponent,
constant zero range step, and malformed duration literals (§15).

### Constraints

A fault inside a `defer` action is subject to runtime logging/collection
policy (§14) and does not become a catchable language value. Public
canonical examples model recoverable failure with `Result`, not faults.

### Lowering / IR Notes

Faults lower to an abort or trap of the current evaluation; within
`concurrent`, an arm fault aborts the whole expression (§15).

## §14 Defer

### Grammar

```ebnf
DeferStmt       ::= "defer" (BlockExpr | CallExpr)
```

### Semantics

`defer` registers cleanup work for the current lexical scope. Deferred actions
run in last-in, first-out order when the scope exits, including exits caused by
`return` or `?`.

Errors inside deferred actions are logged or collected according to runtime
policy; they do not replace an existing returned `Err` in v5.1 canonical
semantics.

### Constraints

A `defer` belongs to the innermost lexical scope in which it appears.

### Lowering / IR Notes

`defer` lowers to scope-finalization handlers or target-language `finally`
mechanisms.

## §15 Concurrent

### Grammar

```ebnf
ConcurrentExpr      ::= ConcurrentJoin | ConcurrentTimeout

ConcurrentJoin      ::= "concurrent" "{" ConcurrentArm+ "}"

ConcurrentTimeout   ::= "concurrent" "(" "timeout" ":" DurationLiteral ")"
                        "{" ConcurrentArm+ "}"
                        "else" BlockExpr

ConcurrentArm       ::= Identifier ":" Expression
DurationLiteral     ::= IntegerLiteral DurationUnit
DurationUnit        ::= "ms" | "s" | "m"
```

### Semantics

`ConcurrentJoin` evaluates all named arms concurrently and returns a
record containing all arm results. `ConcurrentTimeout` evaluates all
named arms concurrently; if every arm completes before the timeout,
the expression returns a record containing all arm results. If the
timeout expires first, the `else` block is evaluated and becomes the
expression result; no partial arm record is exposed.

If any arm faults before timeout, the whole `concurrent` expression
faults (§13a); the `else` block is not evaluated. An arm evaluating to
`Err(e)` is a normal value stored in the corresponding result record
field — it is not a concurrent failure.

Post-timeout cancellation of abandoned arms is runtime policy; faults
from work abandoned by a timeout are not observed by the Topaz
expression.

A `DurationLiteral` is a lexical adjacency form: there must be no
whitespace between the integer literal and the duration unit; `3 s`
is a parse error. Duration units are registry entries for this
literal form, not keywords. Duration literals are part of v5.1 only
for the `concurrent` timeout clause; general duration arithmetic
remains a standard-library matter.

### Constraints

The `else` fallback exists only in the timeout form; the grammar
admits neither `concurrent { ... } else { ... }` without a timeout nor
`concurrent(timeout: d) { ... }` without `else`. Public canonical
examples should prefer the timeout form with explicit `else`;
spec-only or advanced examples may use `ConcurrentJoin` when a plain
parallel join is the point.

Concurrent arm identifiers in a single `concurrent` expression must be unique;
duplicate arm names are a static error.

The successful result type of a `concurrent` expression is a record with one
field per arm, preserving the arm names and using each arm expression's result
type as that field's type. In `ConcurrentTimeout`, the `else` block result must
be compatible with the successful arm-record result under the same branch-result
compatibility rule used for `if` and `match`; otherwise the expression is a
static error.

Migration note (tracked semantic change): this section deliberately
rewrites the v5.0 wording "if the concurrent block fails or times
out". In v5.1, `else` is timeout-only; recoverable failure is an
`Err` arm value, and programmer error is a fault. Downstream
documentation paraphrasing the old wording must be updated (Step 7).

Automatic async without explicit `concurrent` remains outside v5.1
language semantics (§19). Property-style duration forms such as
`.days` remain non-canonical.

### Lowering / IR Notes

Concurrent arms lower to runtime tasks with a join operation and timeout guard.
The result record preserves arm names.

## §16 String Templates

### Grammar

```ebnf
TaggedTemplate  ::= TemplateTag (SingleLineString | MultiLineString)
TemplateTag     ::= "p" | "r" | "sh" | "sql"
```

### Semantics

All v5.1 tagged templates use the double-quoted string literal delimiter and
standard `{expr}` interpolation from §1.

`p"..."` constructs a path template with platform normalization.

`r"..."` constructs a regex template with reduced escaping.

`sh"..."` constructs a shell template value using safe interpolation metadata.
It does not by itself force execution.

`sql"..."` constructs a SQL template in which interpolations become query
parameters. Direct text insertion of interpolated values is forbidden.

Tagged-template recognition is lexical adjacency (§1). The parser then
accepts the tag only if it is in the v5.1 canonical tag registry:
`p`, `r`, `sh`, `sql`. Future versions may add tags to the registry
without changing the lexical rule.

### Constraints

Backtick tagged templates such as `sql` followed by a backtick-delimited body
are not v5.1 canonical syntax. `${expr}` interpolation is not canonical Topaz
syntax. Use `{expr}`.

`html"..."` and HTML tagged templates are deferred and not v5.1 canonical.

Tagged templates compose with both string forms: `sql"""..."""`,
`sh"""..."""`, and the other registry tags are canonical. All other
§16 rules (parameter binding for `sql`, safety metadata for `sh`,
registry validation) apply unchanged to the multiline form.

User-defined template tags are deferred (§20). Tags are not keywords;
outside the adjacency form, `p`, `r`, `sh`, and `sql` are ordinary
identifiers. v5.1 does not introduce juxtaposition calls.

### Lowering / IR Notes

Tagged templates lower to typed template nodes containing the raw string,
interpolation spans, and tag kind. `sql"..."` lowers to parameterized query
metadata.

## §17 Modules and Visibility

### Grammar

N/A for v5.1.

### Semantics

v5.1 public canonical Topaz is specified as a single-file language surface. A
module/import/export system is explicitly deferred.

### Constraints

Canonical v5.1 examples must not invent `import`, `export`, `use`, package, or
module syntax. Interop documents may show target-language module systems only
when explicitly labeled as interop.

### Lowering / IR Notes

Module lowering is out of scope for v5.1.

**Appendix (informative): module design notes for v5.2.** The v5.2
module design track must answer, at minimum: path grammar and
resolution; compilation units and check ordering; initialization
order for top-level statements in imported files; cycle policy
(type-only and value cycles); type/value namespace policy; aliasing
and name-collision handling; the standard-library import story; and
package roots, re-exports, and tooling-facing layout. `import`,
`export`, and `use` remain forbidden as native syntax (§20) and are
not reserved keywords; future module syntax should prefer contextual
keywords.

## §18 Closures

### Grammar

Closure syntax is lambda syntax from §7.

### Semantics

Lambdas capture lexically visible bindings. Captured immutable bindings are
read-only inside the closure. Captured mutable bindings preserve the same
binding cell and may be assigned only when mutation is valid under §4.

Captured bindings live at least as long as the closure value that references
them.

### Constraints

Move-capture syntax, Rust-style closure traits, and explicit capture lists are
not v5.1 Topaz.

### Lowering / IR Notes

Closures lower to function values plus an environment record containing captured
bindings.

## §19 Async Model

### Grammar

N/A beyond §15.

### Semantics

v5.1 defines parallel work only through `concurrent` (§15). It does not define
`async`, `await`, implicit async lifting, or automatic async behavior for I/O.

### Constraints

Public v5.1 examples must not use `async` / `await` or claim automatic async as
a language semantic.

### Lowering / IR Notes

General async lowering is out of scope for v5.1.

## §20 Reserved and Forbidden Forms

### Grammar

N/A.

### Semantics

This section records forms that are intentionally not canonical v5.1 Topaz.

### Constraints

Forbidden in canonical v5.1 examples and public grammar references:

- `mut let`.
- `[T]` as a public collection type.
- `function(T) -> U` in type positions.
- `args: ...T` variadic parameters (canonical form is `...args: T`).
- `[head, ...tail]` list rest patterns (`...rest` in patterns
  generally); pattern rest is `..` (§6).
- JavaScript-style record spread `{ ...value }`.
- Fixed-arity tuple spread and iterable spread (`...` spread is
  `Array`-only and variadic-tail-only, §5/§9).
- `this`, `&self`, `&mut self`, implicit receiver declarations, and
  `struct` method blocks as native Topaz syntax.
- Rust interop tokens (`use crate::`, `::`, `#[derive(...)]`,
  `Vec<T>`, `&str`, `where T: ...`, `impl Fn`, `FnMut`, `move ||`)
  in canonical Topaz examples.
- `++`, `--`, and `**=`.
- Backtick tagged templates and `${expr}` interpolation.
- `html"..."` and HTML tagged templates (deferred).
- User-defined template tags (deferred; registry is `{p, r, sh,
sql}`, §16).
- `assert` as a keyword or statement form (the test-profile stdlib
  function is §22.4).
- `try` keyword expressions or statement forms. Postfix Result propagation
  `expr?` remains canonical (§13); the deferred form is the separate `try`
  surface.
- `import` / `export` / `use` module syntax (deferred to v5.2;
  not reserved keywords).
- A sole-underscore identifier `_` as a binding name (§1).
- String indexing `s[i]`, string `.length`, string slicing syntax,
  and grapheme APIs as core semantics (deferred; §1, §22).
- Explicit generic call-site type arguments such as `f<int>(x)`;
  generic bounds, constraints, interfaces, user-implementable
  protocols, and variance annotations (deferred).
- Recursive alias cycles such as `type Node = { next: Node }` or mutually
  recursive aliases. They are static errors in v5.1; recursive/inductive type
  aliases are deferred.
- Labeled `break` and `break value` (deferred).
- Optional-property pipe sugar `|> ?.field` (deferred).
- `x in map` (use `x in map.keys`, §9); `Map`/`Set` equality
  (deferred, §2).
- Anonymous `function(...) -> T { ... }` expressions (use lambdas).
- `async` / `await` and automatic-async claims (§19); catchable
  faults, panic keywords, and throw/catch (§13a).

### Lowering / IR Notes

N/A.

## §21 IR/Lowering Notes

### Grammar

N/A.

### Semantics

This section is informative. Implementations may choose different internal
representations if observable v5.1 semantics are preserved.

### Constraints

Lowering notes must not be cited as permission to use syntax absent from
normative sections.

### Lowering / IR Notes

Summary:

- Optional chaining lowers to optional/null checks.
- Record update lowers to copy-plus-set.
- Range `by` lowers to stride.
- List patterns lower to length checks and slice/index binding.
- Pipeline and placeholder sugar lower to lambda or call forms.
- `in` lowers to membership checks appropriate to the container.
- `??=` lowers to if-null/none assignment.
- `?` lowers to Result branch and early return.
- `defer` lowers to scope finalization.
- `concurrent` lowers to runtime tasks plus join and fallback.
- Tagged templates lower to typed template metadata with interpolation spans.

## §22 Standard Library Surface

### Grammar

N/A. Signatures below use declaration-style notation. Receiver-form entries
(`arr.get(...)`, `m.insert(...)`, `s.scalars()`) and property entries
(`arr.length`, `m.keys`) are descriptive surface notation for built-in members
accessed through ordinary `MemberExpr`/`CallExpr` syntax. They do not introduce
user-definable receiver declarations, method blocks, or implicit `this`/`self`
syntax (§8, §20).

### §22.1 Prelude

```topaz
Some<T>(value: T) -> Option<T>
None: Option<T>
Ok<T, E>(value: T) -> Result<T, E>
Err<T, E>(error: E) -> Result<T, E>
```

`None` is a polymorphic constructor value, not an ordinary variable.

Contextual-typing rule: any expression that still contains unsolved
type variables after local inference requires a contextual type — an
annotated binding, parameter type, declared return type, record field
expectation, collection element type, match-arm expected type, or
another expected-type site defined by the type checker. Without
context, the expression is a static error. This applies to (at least)
`None`, `Ok(...)` (unsolved `E`), `Err(...)` (unsolved `T`), `[]`,
`Array.of()`, `Map.new()`, and `Set.of()`.

`Iterable<T>` is a prelude protocol name used only by standard-library
signatures in v5.1. The iterable types are exactly the language
iteration types of §10. It is not user-implementable; user-defined
iterable protocols are deferred (§20).

### §22.2 Core minimum

```topaz
print(value: string) -> ()
toInt(text: string) -> Option<int>

s.scalars() -> Array<string>            // s: string; single-scalar strings

Array.of<T>(...items: T) -> Array<T>
arr.push(x: T) -> ()
arr.get(i: int) -> Option<T>            // None for i < 0 or i >= arr.length
arr.length: int

Map.new<K, V>() -> Map<K, V>
m.insert(k: K, v: V) -> ()
m.get(k: K) -> Option<V>                // None when the key is absent
m.remove(k: K) -> Option<V>             // Some(old) if present, else None
m.keys: Array<K>

Set.of<T>(...items: T) -> Set<T>
s.add(x: T) -> ()
s.remove(x: T) -> bool                  // true iff an element was removed

map<T, U>(xs: Iterable<T>, f: (T) -> U) -> Array<U>
filter<T>(xs: Iterable<T>, f: (T) -> bool) -> Array<T>
reduce<T, U>(xs: Iterable<T>, initial: U, f: (U, T) -> U) -> U
```

`arr.get` is the canonical non-faulting read; direct indexing `arr[i]`
faults on out-of-bounds or negative indices (§13a). `print` is
string-only; non-string values are printed via interpolation. Strings
expose no `.length`; use `s.scalars().length` (implementations may
optimize).

`m.keys` evaluates to an `Array<K>` snapshot of the map's keys in key insertion
order. Mutating the map later does not mutate an already produced keys array.
`Map.new` and `Set.of` inherit the comparability constraints for map keys and
set elements from §9.

### §22.3 File minimum

```topaz
open(path: string) -> Result<File, string>
file.read() -> Result<string, string>
file.write(s: string) -> Result<(), string>
file.close() -> ()
```

Append modes, binary I/O, buffering, permissions, and async I/O are not
specified in v5.1.

`File` is an opaque standard-library resource type. File values are not
comparable (§2). Behavior after `file.close()` beyond the signatures listed
here is runtime/library policy unless a future standard-library ADR specifies
it.

### §22.4 Reserved canonical names and test profile

`JSON.parse` and `regex.match` are reserved standard-library names.
They are not part of the typed minimum until `JSONValue`, `Regex`, and
`Match` are specified; examples may mention them only where the return
shape is irrelevant, with a deferral note.

```topaz
assert(condition: bool, message: string = "") -> ()
```

`assert` is available only under the test profile (PROFILES); its
failure is a fault (§13a). It is not a keyword and not part of any
public profile surface. `assertEq` is not specified in v5.1.

### Constraints

This section is the normative minimum used by canonical examples.
Names not listed here and not reserved above are illustrative
placeholders and must be marked as such where used.

### Lowering / IR Notes

Standard-library calls lower to runtime or target-library calls
preserving the typing above.
