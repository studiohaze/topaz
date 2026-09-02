# docpkg 0.1.0

- Entry: `src/lib.tpz`
- Language: `5.4`

## Modules

### `src.lib`

#### Values
- `greet`: `(User) -> string` (1 required; params `user`)
  Build a greeting for a user.

#### Records
- `User`
  A user that can be rendered.
  - `name`: `string`

#### Newtypes
- `UserId` = `int`
  Stable numeric user identity.

#### Conformances
- `User`: `Show`
