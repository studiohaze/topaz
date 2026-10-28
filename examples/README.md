# Topaz 5.21.0 examples

Run these commands from the repository root with Topaz 5.21.0.

## Variables and output

Bind values and insert them into a string.

[Source](../docs/samples/learn/first-program.tpz)

```sh
topaz run docs/samples/learn/first-program.tpz
```

Expected output

```text
Topaz basics: 25 minutes
```

## Functions and state

Call a typed function and update a mutable variable.

[Source](../docs/samples/learn/values-functions.tpz)

```sh
topaz run docs/samples/learn/values-functions.tpz
```

Expected output

```text
Topaz basics: 25 minutes left
sessions: 1
```

## Records and control flow

Use record defaults, match and a loop to total unfinished tasks.

[Source](../docs/samples/learn/data-control.tpz)

```sh
topaz run docs/samples/learn/data-control.tpz
```

Expected output

```text
Run first program: done
Build application: focused
remaining: 25 minutes
```

## Pipelines

Pass values between functions and use `_` to place the input in an argument slot.

[Source](../docs/samples/language/pipelines-placeholders.tpz)

```sh
topaz run docs/samples/language/pipelines-placeholders.tpz
```

Expected output

```text
21 true
```

[Language guide](https://topaz.ooo/v5.21/en/docs/getting-started)
