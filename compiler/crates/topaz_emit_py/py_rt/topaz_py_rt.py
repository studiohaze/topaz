from __future__ import annotations

from dataclasses import dataclass, field as dataclass_field, make_dataclass
import csv
import hashlib
import hmac
import inspect
import io
import json
import math
from pathlib import Path
import re
import struct
import sys
import tomllib
import types
from urllib.parse import parse_qsl, urlsplit


INT_MIN = -(1 << 63)
INT_MAX = (1 << 63) - 1
I128_MIN = -(1 << 127)
I128_MAX = (1 << 127) - 1
U32_MAX = (1 << 32) - 1
U64_MAX = (1 << 64) - 1
_NO_TRACE_VALUE = object()
# These match the shared runtime comparator budget for bounded cyclic and deeply nested values.
STRUCT_FUEL = 100_000
STRUCT_DEPTH = 128


class _StructBudget:
    __slots__ = ("fuel",)

    def __init__(self, fuel: int = STRUCT_FUEL) -> None:
        self.fuel = fuel

    def consume(self, depth: int, span: tuple[int, int, int]) -> None:
        if self.fuel <= 0 or depth > STRUCT_DEPTH:
            tpz_fault("TPZ5007", "comparison exceeded the structural budget (cyclic value?)", span)
        self.fuel -= 1


class _TpzUnit:
    __slots__ = ()

    def __repr__(self) -> str:
        return "()"


class _TpzNull:
    __slots__ = ()

    def __repr__(self) -> str:
        return "null"


TPZ_UNIT = _TpzUnit()
TPZ_NULL = _TpzNull()
TPZ_ANONYMOUS_VARIADIC_TAIL_KW = "__tpz_variadic_tail__"


@dataclass(frozen=True, slots=True)
class TpzFault(Exception):
    code: str
    message: str
    span: tuple[int, int, int]

    def to_json(self) -> dict[str, object]:
        file, lo, hi = self.span
        return {
            "code": self.code,
            "message": self.message,
            "span": {"file": file, "lo": lo, "hi": hi},
        }


@dataclass(frozen=True, slots=True)
class Some:
    value: object


@dataclass(frozen=True, slots=True)
class Ok:
    value: object


@dataclass(frozen=True, slots=True)
class Err:
    value: object


@dataclass(frozen=True, slots=True)
class TpzNewtype:
    newtype_id: str
    value: object
    method_identity: str | None = dataclass_field(default=None, compare=False, repr=False)
    declaration_identity: str | None = dataclass_field(default=None, repr=False)


@dataclass(frozen=True, slots=True)
class TpzEnum:
    enum_id: str
    variant: str
    variant_index: int
    payloads: tuple[object, ...]
    method_identity: str | None = dataclass_field(default=None, compare=False, repr=False)
    declaration_identity: str | None = dataclass_field(default=None, repr=False)


@dataclass(frozen=True, slots=True)
class TpzReturn(Exception):
    value: object


@dataclass(frozen=True, slots=True)
class TpzLoopBreak(Exception):
    label: str | None
    value: object


@dataclass(frozen=True, slots=True)
class TpzLoopContinue(Exception):
    label: str | None


@dataclass(frozen=True, slots=True)
class TpzFile:
    host: object
    handle: int


@dataclass(frozen=True, slots=True)
class TpzBytes:
    data: bytes


@dataclass(slots=True, eq=False)
class TpzByteBuffer:
    data: bytearray


# eq=False: §16 templates are NEVER structurally equal (the shared Rust
# comparator yields false for template operands, and the checker rejects the
# comparison statically), so the dataclass must not synthesize structural
# __eq__/__hash__ that raw Python paths could observe.
@dataclass(frozen=True, slots=True, eq=False)
class TpzTemplate:
    tag: str
    parts: tuple[str, ...]
    values: tuple[object, ...]
    normalized: str


@dataclass(frozen=True, slots=True)
class TpzFloatKey:
    bits: int
    value: float

    def __eq__(self, other: object) -> bool:
        if not isinstance(other, TpzFloatKey):
            return False
        if math.isnan(self.value) or math.isnan(other.value):
            return False
        return self.value == other.value

    def __hash__(self) -> int:
        if math.isnan(self.value):
            return hash(("f64", self.bits))
        return hash(("f64", self.value))


@dataclass(slots=True)
class TpzMap:
    entries: list[tuple[object, object]]


@dataclass(slots=True)
class TpzSet:
    items: list[object]

    def __iter__(self):
        return (_key_to_value(item) for item in self.items)


@dataclass(frozen=True, slots=True)
class TpzRange:
    lo: int
    hi: int
    inclusive: bool
    step: int

    def __iter__(self):
        if self.step == 0:
            tpz_fault("TPZ4003", "range step must not be zero (§10)", (0, 0, 0))
        value = self.lo
        while True:
            if self.step > 0:
                within = value <= self.hi if self.inclusive else value < self.hi
            else:
                within = value >= self.hi if self.inclusive else value > self.hi
            if not within:
                return
            yield value
            next_value = value + self.step
            if next_value < INT_MIN or next_value > INT_MAX:
                return
            value = next_value


@dataclass(frozen=True, slots=True)
class TpzComposed:
    first: object
    second: object
    span: tuple[int, int, int]

    def __call__(self, *args, **kwargs):
        return self.call_with_span(args, kwargs, self.span)

    def call_with_span(self, args: tuple[object, ...], kwargs: dict[str, object], span: tuple[int, int, int]) -> object:
        mid = tpz_call(self.first, args, kwargs, span)
        return tpz_call(self.second, (mid,), {}, span)

    def __call_cooperative__(self, *args, **kwargs):
        mid = yield from tpz_call_cooperative(self.first, args, kwargs, self.span)
        return (yield from tpz_call_cooperative(self.second, (mid,), {}, self.span))


@dataclass(frozen=True, slots=True)
class TpzHostCallable:
    fn: object
    host: object
    cooperative_fn: object | None = None
    variadic_py_name: str | None = None

    def _rewrite_anonymous_variadic_tail(
        self,
        args: tuple[object, ...],
        kwargs: dict[str, object],
    ) -> tuple[tuple[object, ...], dict[str, object]]:
        if TPZ_ANONYMOUS_VARIADIC_TAIL_KW not in kwargs:
            return args, kwargs
        tail = kwargs.pop(TPZ_ANONYMOUS_VARIADIC_TAIL_KW)
        if self.variadic_py_name is not None:
            kwargs[self.variadic_py_name] = tail
            return args, kwargs
        # Fallback for non-Topaz Python callables that satisfy a variadic function type.
        return args + tuple(tail), kwargs

    def __call__(self, *args, **kwargs):
        args, kwargs = self._rewrite_anonymous_variadic_tail(args, kwargs)
        return self.fn(self.host, *args, **kwargs)

    def __call_cooperative__(self, *args, **kwargs):
        args, kwargs = self._rewrite_anonymous_variadic_tail(args, kwargs)
        if self.cooperative_fn is not None:
            return (yield from self.cooperative_fn(self.host, *args, **kwargs))
        return self.fn(self.host, *args, **kwargs)


@dataclass(frozen=True, slots=True)
class TpzCooperativeCallable:
    fn: object
    cooperative_fn: object

    def __call__(self, *args, **kwargs):
        return self.fn(*args, **kwargs)

    def __call_cooperative__(self, *args, **kwargs):
        result = self.cooperative_fn(*args, **kwargs)
        if isinstance(result, types.GeneratorType):
            return (yield from result)
        return result


@dataclass(frozen=True, slots=True)
class TpzBoundUserMethod:
    method: object
    receiver: object
    prepend_receiver: bool

    def __call__(self, *args, **kwargs):
        call_args = (self.receiver, *args) if self.prepend_receiver else args
        return tpz_call(self.method, call_args, kwargs, (0, 0, 0))

    def __call_cooperative__(self, *args, **kwargs):
        call_args = (self.receiver, *args) if self.prepend_receiver else args
        return (yield from tpz_call_cooperative(self.method, call_args, kwargs, (0, 0, 0)))


@dataclass(frozen=True, slots=True)
class TpzExternFunction:
    module: str
    function: str
    span: tuple[int, int, int]

    def __call__(self, host: object, *args):
        try:
            return host.extern_call(self.module, self.function, args)
        except ValueError as error:
            tpz_fault("TPZ5032", str(error), self.span)


def tpz_concurrent_join(arms: list[tuple[str, object]]) -> list[object]:
    pending: list[tuple[int, str, object]] = []
    results: list[object] = [TPZ_UNIT for _ in arms]
    for index, (name, thunk) in enumerate(arms):
        pending.append((index, name, thunk()))
    while pending:
        cursor = 0
        while cursor < len(pending):
            index, _name, gen = pending[cursor]
            try:
                next(gen)
                cursor += 1
            except StopIteration as done:
                results[index] = done.value
                pending.pop(cursor)
    return results


def _tpz_drive_generator(thunk: object) -> object:
    gen = thunk()
    while True:
        try:
            next(gen)
        except StopIteration as done:
            return done.value


def tpz_concurrent_join_timeout(
    arms: list[tuple[str, object]], timeout_ms: int, else_thunk: object
) -> tuple[bool, object]:
    pending: list[tuple[int, str, object]] = []
    results: list[object] = [TPZ_UNIT for _ in arms]
    for index, (name, thunk) in enumerate(arms):
        pending.append((index, name, thunk()))
    while pending:
        cursor = 0
        while cursor < len(pending):
            index, _name, gen = pending[cursor]
            try:
                next(gen)
                cursor += 1
            except StopIteration as done:
                results[index] = done.value
                pending.pop(cursor)
            except TpzFault:
                if timeout_ms > 0:
                    raise
                return (False, _tpz_drive_generator(else_thunk))
            # Differential execution uses a frozen zero-valued clock: 0ms is
            # expired after each textual arm quantum, while a positive timeout
            # never expires. A completed sole arm therefore still wins at 0ms.
            if pending and timeout_ms == 0:
                return (False, _tpz_drive_generator(else_thunk))
    return (True, results)


@dataclass(frozen=True, slots=True)
class ExternSandboxPolicy:
    module: str
    kind: str
    artifact_path: str | None
    fuel: int | None
    memory_bytes: int | None


@dataclass(frozen=True, slots=True)
class TpzJsonNumber:
    lexeme: str
    int_value: int | None


@dataclass(frozen=True, slots=True)
class TpzJson:
    kind: str
    value: object = None


@dataclass(frozen=True, slots=True)
class TpzJsonParseErrorRecord:
    _t_636f6c756d6e: int
    _t_6c696e65: int
    _t_6d657373616765: str


@dataclass(frozen=True, slots=True)
class TpzRegex:
    pattern: str
    compiled: re.Pattern[str]


@dataclass(frozen=True, slots=True)
class TpzToml:
    value: object


@dataclass(frozen=True, slots=True)
class TpzUrl:
    canonical: str
    scheme_value: str
    host_value: str | None
    path_value: str
    query_value: tuple[tuple[str, str], ...]
    fragment_value: str | None


_RECORD_CLASS_CACHE: dict[
    tuple[str | None, str | None, tuple[tuple[str, str], ...]], type
] = {}


class ExternReplayStore:
    def __init__(
        self,
        entries: dict[tuple[str, str, str], object] | None = None,
        policies: dict[str, ExternSandboxPolicy] | None = None,
    ) -> None:
        self._entries: dict[tuple[str, str, str], object] = dict(entries or {})
        self._policies = policies

    @classmethod
    def parse_jsonl(
        cls,
        text: str,
        policies_json: str | None = None,
    ) -> ExternReplayStore:
        entries: dict[tuple[str, str, str], object] = {}
        for index, raw in enumerate(text.splitlines(), start=1):
            line = raw.strip()
            if not line:
                continue
            try:
                node = json.loads(line)
            except json.JSONDecodeError as error:
                raise ValueError(
                    "extern replay line "
                    + str(index)
                    + ": invalid JSON at line "
                    + str(error.lineno)
                    + ", column "
                    + str(error.colno)
                    + ": "
                    + error.msg
                ) from None
            if not isinstance(node, dict):
                raise ValueError("extern replay line " + str(index) + ": row must be an object")
            if set(node.keys()) != {"module", "function", "args", "result"}:
                raise ValueError(
                    "extern replay line "
                    + str(index)
                    + ": expected fields args, function, module, result"
                )
            module = node["module"]
            function = node["function"]
            args_node = node["args"]
            if not isinstance(module, str):
                raise ValueError("extern replay line " + str(index) + ": `module` must be a string")
            if not isinstance(function, str):
                raise ValueError("extern replay line " + str(index) + ": `function` must be a string")
            if not isinstance(args_node, list):
                raise ValueError("extern replay line " + str(index) + ": `args` must be an array")
            args = tuple(_abi_decode_value(arg, "line " + str(index) + ".args[" + str(i) + "]", 0) for i, arg in enumerate(args_node))
            result = _abi_decode_value(node["result"], "line " + str(index) + ".result", 0)
            key = (module, function, _abi_args_encode(args))
            if key in entries:
                raise ValueError(
                    "extern replay line "
                    + str(index)
                    + ": duplicate row for `"
                    + module
                    + "."
                    + function
                    + "` with canonical ABI args `"
                    + key[2]
                    + "`"
                )
            entries[key] = result
        return cls(entries, _parse_extern_sandbox_policies(policies_json))

    def call(self, module: str, function: str, args: tuple[object, ...]) -> object:
        args_json = _abi_args_encode(args)
        key = (module, function, args_json)
        policy = None
        if self._policies is not None:
            policy = self._policies.get(module)
            if policy is None:
                raise ValueError("extern sandbox policy for `" + module + "` is not available")
            _validate_extern_sandbox_policy(policy)
        if key not in self._entries:
            raise ValueError(
                "extern replay has no row for `"
                + module
                + "."
                + function
                + "` with canonical ABI args `"
                + args_json
                + "`"
            )
        result = self._entries[key]
        if policy is not None:
            _enforce_extern_replay_budget(policy, module, function, args, args_json, result)
        return result


class Host:
    def __init__(
        self,
        stdin_text: str,
        files: dict[str, str] | None = None,
        extern_replay_jsonl: str | None = None,
        extern_sandbox_policies_json: str | None = None,
    ) -> None:
        self._stdin_text = stdin_text
        self.stdout: list[str] = []
        self.defer_errors: list[dict[str, object]] = []
        self._files: dict[str, bytes] = {
            path: content.encode("utf-8") for path, content in (files or {}).items()
        }
        self._open: dict[int, tuple[str, bool]] = {}
        self._next_handle = 0
        self._extern_replay = ExternReplayStore.parse_jsonl(
            extern_replay_jsonl or "",
            extern_sandbox_policies_json,
        )

    def input(self) -> str:
        return self._stdin_text

    def trace_files(self) -> list[dict[str, object]]:
        return [
            {
                "path": path,
                "content": tpz_trace_value(content.decode("utf-8", "replace")),
            }
            for path, content in sorted(self._files.items())
        ]

    def print(self, value: object, span: tuple[int, int, int]) -> object:
        if not isinstance(value, str):
            tpz_fault("TPZ5001", "`print` is string-only; interpolate instead (§22.2)", span)
        self.stdout.append(value)
        return TPZ_UNIT

    def open_file(self, path: object, span: tuple[int, int, int]) -> Ok | Err:
        if not isinstance(path, str):
            tpz_fault("TPZ5001", "`open` takes a `string`, found `" + tpz_kind(path) + "`", span)
        if path not in self._files:
            return Err("cannot open `" + path + "`: not found")
        self._next_handle += 1
        self._open[self._next_handle] = (path, False)
        return Ok(TpzFile(self, self._next_handle))

    def read_file(self, handle: int) -> Ok | Err:
        entry = self._open.get(handle)
        if entry is None:
            return Err("file is not open")
        path, closed = entry
        if closed:
            return Err("file is closed")
        if path not in self._files:
            return Err("cannot read `" + path + "`")
        try:
            return Ok(self._files[path].decode("utf-8"))
        except UnicodeError as error:
            return Err(str(error))

    def write_file(self, handle: int, value: object, span: tuple[int, int, int]) -> Ok | Err:
        if not isinstance(value, str):
            tpz_fault(
                "TPZ5001",
                "`file.write` takes a `string`, found `" + tpz_kind(value) + "`",
                span,
            )
        entry = self._open.get(handle)
        if entry is None:
            return Err("file is not open")
        path, closed = entry
        if closed:
            return Err("file is closed")
        self._files[path] = value.encode("utf-8")
        return Ok(TPZ_UNIT)

    def close_file(self, handle: int) -> None:
        entry = self._open.get(handle)
        if entry is not None:
            path, _closed = entry
            self._open[handle] = (path, True)

    def fs_read_text(self, path: str) -> Ok | Err:
        opened = self.open_file(path, (0, 0, 0))
        if isinstance(opened, Err):
            return opened
        read = self.read_file(opened.value.handle)
        self.close_file(opened.value.handle)
        return read

    def fs_write_text(self, path: str, text: str) -> Ok | Err:
        opened = self.open_file(path, (0, 0, 0))
        if isinstance(opened, Err):
            return opened
        written = self.write_file(opened.value.handle, text, (0, 0, 0))
        self.close_file(opened.value.handle)
        return written

    def fs_read_bytes(self, path: str) -> Ok | Err:
        if path not in self._files:
            return Err("cannot read `" + path + "`")
        return Ok(TpzBytes(self._files[path]))

    def fs_write_bytes(self, path: str, value: TpzBytes) -> Ok | Err:
        self._files[path] = value.data
        return Ok(TPZ_UNIT)

    def fs_list(self, path: str) -> Ok | Err:
        if path == "" or path == ".":
            prefix = ""
        elif path.endswith("/"):
            prefix = path
        else:
            prefix = path + "/"
        entries: dict[str, object] = {}
        for file_path, contents in self._files.items():
            if not file_path.startswith(prefix):
                continue
            rest = file_path[len(prefix) :]
            if rest == "":
                continue
            if "/" in rest:
                dirname = rest.split("/", 1)[0]
                entries[dirname] = _fs_dir_entry(dirname, "directory", None)
            else:
                entries[rest] = _fs_dir_entry(rest, "file", len(contents))
        if not entries and path not in self._files:
            return Err("cannot list `" + path + "`")
        return Ok([entries[name] for name in sorted(entries)])

    def extern_call(self, module: str, function: str, args: tuple[object, ...]) -> object:
        return self._extern_replay.call(module, function, args)

    def trace_ok(self, value: object = _NO_TRACE_VALUE) -> str:
        return tpz_trace_line("ok", self.stdout, self.trace_files(), self.defer_errors, None, value)

    def trace_fault(self, fault: TpzFault) -> str:
        return tpz_trace_line(
            "fault",
            self.stdout,
            self.trace_files(),
            self.defer_errors,
            fault.to_json(),
        )

    def defer_error(self, rendered: str, fault: TpzFault | None = None) -> None:
        entry: dict[str, object] = {"rendered": rendered}
        if fault is not None:
            entry["fault"] = fault.to_json()
        self.defer_errors.append(entry)


class DeploymentHost(Host):
    def __init__(
        self,
        stdin_text: str,
        base: str,
        read_roots: list[str],
        write_roots: list[str],
        extern_replay_jsonl: str | None = None,
        extern_sandbox_policies_json: str | None = None,
    ) -> None:
        super().__init__(
            stdin_text,
            None,
            extern_replay_jsonl,
            extern_sandbox_policies_json,
        )
        self._base = Path(base).resolve()
        self._read_roots = self._deployment_roots(read_roots)
        self._write_roots = self._deployment_roots(write_roots)

    def _deployment_roots(self, roots: list[str]) -> tuple[Path, ...]:
        resolved: list[Path] = []
        for raw in roots:
            root = Path(raw)
            if not root.is_absolute():
                root = self._base / root
            root = root.resolve()
            if not self._contains(self._base, root):
                raise ValueError(
                    "package fs capability root `"
                    + raw
                    + "` resolves outside runtime base `"
                    + str(self._base)
                    + "`"
                )
            resolved.append(root)
        return tuple(resolved)

    @staticmethod
    def _contains(root: Path, target: Path) -> bool:
        try:
            target.relative_to(root)
            return True
        except ValueError:
            return False

    def _resolve_path(self, path: str) -> Path:
        target = Path(path)
        if not target.is_absolute():
            target = self._base / target
        return target.resolve()

    def _resolve_open(self, path: str) -> tuple[Path, bool, bool] | str:
        target = self._resolve_path(path)
        read_allowed = any(self._contains(root, target) for root in self._read_roots)
        write_allowed = any(self._contains(root, target) for root in self._write_roots)
        if not read_allowed and not write_allowed:
            return "cannot open `" + path + "`: not permitted by package fs capabilities"
        return target, read_allowed, write_allowed

    def print(self, value: object, span: tuple[int, int, int]) -> object:
        if not isinstance(value, str):
            tpz_fault("TPZ5001", "`print` is string-only; interpolate instead (§22.2)", span)
        sys.stdout.write(value + "\n")
        return TPZ_UNIT

    def open_file(self, path: object, span: tuple[int, int, int]) -> Ok | Err:
        if not isinstance(path, str):
            tpz_fault("TPZ5001", "`open` takes a `string`, found `" + tpz_kind(path) + "`", span)
        resolved = self._resolve_open(path)
        if isinstance(resolved, str):
            return Err(resolved)
        target, read_allowed, write_allowed = resolved
        if not target.exists():
            return Err("cannot open `" + path + "`: not found")
        self._next_handle += 1
        self._open[self._next_handle] = (str(target), False, read_allowed, write_allowed)
        return Ok(TpzFile(self, self._next_handle))

    def read_file(self, handle: int) -> Ok | Err:
        entry = self._open.get(handle)
        if entry is None:
            return Err("file is not open")
        path, closed, read_allowed, _write_allowed = entry
        if closed:
            return Err("file is closed")
        if not read_allowed:
            return Err("file is not readable by package fs capabilities")
        target = self._resolve_path(path)
        if not any(self._contains(root, target) for root in self._read_roots):
            return Err("file resolves outside package fs capabilities")
        try:
            return Ok(target.read_text(encoding="utf-8"))
        except (OSError, UnicodeError) as error:
            return Err(str(error))

    def write_file(self, handle: int, value: object, span: tuple[int, int, int]) -> Ok | Err:
        if not isinstance(value, str):
            tpz_fault(
                "TPZ5001",
                "`file.write` takes a `string`, found `" + tpz_kind(value) + "`",
                span,
            )
        entry = self._open.get(handle)
        if entry is None:
            return Err("file is not open")
        path, closed, _read_allowed, write_allowed = entry
        if closed:
            return Err("file is closed")
        if not write_allowed:
            return Err("file is not writable by package fs capabilities")
        target = self._resolve_path(path)
        if not any(self._contains(root, target) for root in self._write_roots):
            return Err("file resolves outside package fs capabilities")
        try:
            target.write_text(value, encoding="utf-8")
            return Ok(TPZ_UNIT)
        except (OSError, UnicodeError) as error:
            return Err(str(error))

    def close_file(self, handle: int) -> None:
        entry = self._open.get(handle)
        if entry is not None:
            path, _closed, read_allowed, write_allowed = entry
            self._open[handle] = (path, True, read_allowed, write_allowed)

    def fs_read_bytes(self, path: str) -> Ok | Err:
        resolved = self._resolve_open(path)
        if isinstance(resolved, str):
            return Err(resolved.replace("cannot open", "cannot read", 1))
        target, read_allowed, _write_allowed = resolved
        if not read_allowed:
            return Err("cannot read `" + path + "`: not permitted by package fs capabilities")
        try:
            return Ok(TpzBytes(target.read_bytes()))
        except OSError as error:
            return Err(str(error))

    def fs_write_bytes(self, path: str, value: TpzBytes) -> Ok | Err:
        resolved = self._resolve_open(path)
        if isinstance(resolved, str):
            return Err(resolved.replace("cannot open", "cannot write", 1))
        target, _read_allowed, write_allowed = resolved
        if not write_allowed:
            return Err("cannot write `" + path + "`: not permitted by package fs capabilities")
        try:
            target.write_bytes(value.data)
            return Ok(TPZ_UNIT)
        except OSError as error:
            return Err(str(error))

    def fs_list(self, path: str) -> Ok | Err:
        resolved = self._resolve_open(path)
        if isinstance(resolved, str):
            return Err(resolved.replace("cannot open", "cannot list", 1))
        target, read_allowed, _write_allowed = resolved
        if not read_allowed:
            return Err("cannot list `" + path + "`: not permitted by package fs capabilities")
        try:
            entries = []
            for child in sorted(target.iterdir(), key=lambda entry: entry.name):
                stat = child.lstat()
                if child.is_symlink():
                    kind = "symlink"
                    size_bytes = None
                elif child.is_file():
                    kind = "file"
                    size_bytes = stat.st_size
                elif child.is_dir():
                    kind = "directory"
                    size_bytes = None
                else:
                    kind = "other"
                    size_bytes = None
                entries.append(_fs_dir_entry(child.name, kind, size_bytes))
            return Ok(entries)
        except OSError as error:
            return Err(str(error))

    def defer_error(self, rendered: str, fault: TpzFault | None = None) -> None:
        del fault
        sys.stderr.write("deferred action error: " + rendered + "\n")

    def application_exit(self, value: object) -> int:
        if isinstance(value, Ok):
            code = value.value
            if type(code) is int and 0 <= code <= 255:
                return code
            if type(code) is int:
                sys.stderr.write(
                    "topaz: explicit main returned exit code "
                    + str(code)
                    + "; expected 0..255\n"
                )
            else:
                sys.stderr.write(
                    "topaz: explicit main returned `Ok("
                    + tpz_kind(code)
                    + ")`; expected `Ok(int)`\n"
                )
            return 1
        if isinstance(value, Err):
            message = value.value
            if isinstance(message, str):
                sys.stderr.write(message + "\n")
            else:
                sys.stderr.write(
                    "topaz: explicit main returned `Err("
                    + tpz_kind(message)
                    + ")`; expected `Err(string)`\n"
                )
            return 1
        sys.stderr.write(
            "topaz: explicit main returned `"
            + tpz_kind(value)
            + "`; expected `Result<int, string>`\n"
        )
        return 1

    def application_fault(self, fault: TpzFault) -> int:
        sys.stderr.write("topaz fault: " + fault.message + "\n")
        return 1


def tpz_trace_line(
    status: str,
    stdout: list[str],
    files: list[dict[str, object]],
    defer_errors: list[dict[str, object]],
    fault: dict[str, object] | None,
    value: object = _NO_TRACE_VALUE,
) -> str:
    # Trace v1 key order is stable for readable diffs. Consumers parse structurally.
    trace: dict[str, object] = {
        "v": 1,
        "status": status,
        "stdout": stdout,
        "files": files,
        "defer_errors": defer_errors,
        "fault": fault,
    }
    if value is not _NO_TRACE_VALUE:
        trace["value"] = tpz_trace_value(value)
    return json.dumps(
        trace,
        ensure_ascii=False,
        separators=(",", ":"),
    )


def tpz_f64_from_bits(bits: int) -> float:
    if type(bits) is not int or bits < 0 or bits > U64_MAX:
        raise ValueError("f64 bits must be a u64")
    return struct.unpack("<d", struct.pack("<Q", bits))[0]


def tpz_f64_bits(value: object) -> int:
    if type(value) is not float:
        raise TypeError("expected float")
    return struct.unpack("<Q", struct.pack("<d", value))[0]


_CANONICAL_ARITHMETIC_NAN = tpz_f64_from_bits(0x7FF8000000000000)


def _canonicalize_arithmetic_nan(value: float) -> float:
    if math.isnan(value):
        return _CANONICAL_ARITHMETIC_NAN
    return value


def tpz_trace_value(value: object) -> dict[str, object]:
    if type(value) is float:
        return {"f64": f"{tpz_f64_bits(value):016x}"}
    if type(value) is bool:
        return {"bool": value}
    if type(value) is int:
        return {"int": value}
    if isinstance(value, str):
        return {"str": value}
    if value is TPZ_UNIT or value is TPZ_NULL or value is None:
        return {"null": None}
    if isinstance(value, Some):
        return {"some": tpz_trace_value(value.value)}
    if isinstance(value, Ok):
        return {"result": {"ok": tpz_trace_value(value.value)}}
    if isinstance(value, Err):
        return {"result": {"err": tpz_trace_value(value.value)}}
    if isinstance(value, TpzBytes):
        return {"bytes": tpz_bytes_to_hex(value)}
    if _is_topaz_enum(value):
        return {
            "enum": {
                "id": value.enum_id,
                "variant": value.variant,
                "index": value.variant_index,
                "payloads": [tpz_trace_value(item) for item in value.payloads],
            }
        }
    if isinstance(value, TpzMap):
        return {
            "map": [
                {"key": tpz_trace_value(_key_to_value(key)), "value": tpz_trace_value(item)}
                for key, item in value.entries
            ]
        }
    if isinstance(value, TpzSet):
        return {"set": [tpz_trace_value(_key_to_value(key)) for key in value.items]}
    if isinstance(value, TpzRange):
        return {
            "range": {
                "lo": value.lo,
                "hi": value.hi,
                "inclusive": value.inclusive,
                "step": value.step,
            }
        }
    if isinstance(value, list):
        return {"list": [tpz_trace_value(item) for item in value]}
    if _is_topaz_record(value):
        return {
            "record": {
                source_field: tpz_trace_value(getattr(value, py_field))
                for py_field, source_field in sorted(
                    value.__topaz_record_fields__, key=lambda item: item[1]
                )
            }
        }
    raise TypeError("unsupported trace value")


def tpz_format_f64(value: object) -> str:
    if type(value) is not float:
        raise TypeError("expected float")
    if math.isnan(value):
        return "NaN"
    if math.isinf(value):
        return "inf" if value > 0.0 else "-inf"
    if value.is_integer() and abs(value) < 1e15:
        return f"{value:.1f}"
    return _tpz_expand_shortest_float_repr(repr(value))


def _tpz_expand_shortest_float_repr(text: str) -> str:
    sign = ""
    if text.startswith("-"):
        sign = "-"
        text = text[1:]
    lower = text.lower()
    if "e" not in lower:
        if text.endswith(".0"):
            text = text[:-2]
        return sign + text
    mantissa, exponent_text = lower.split("e", 1)
    exponent = int(exponent_text)
    if "." in mantissa:
        whole, frac = mantissa.split(".", 1)
    else:
        whole, frac = mantissa, ""
    digits = whole + frac
    decimal_pos = len(whole) + exponent
    if decimal_pos <= 0:
        body = "0." + ("0" * (-decimal_pos)) + digits
    elif decimal_pos >= len(digits):
        body = digits + ("0" * (decimal_pos - len(digits)))
    else:
        body = digits[:decimal_pos] + "." + digits[decimal_pos:]
    return sign + body


def tpz_fault(code: str, message: str, span: tuple[int, int, int]) -> None:
    raise TpzFault(code, message, span)


def tpz_try(value: object, span: tuple[int, int, int]) -> object:
    if isinstance(value, Ok):
        return value.value
    if isinstance(value, Err):
        raise TpzReturn(value)
    tpz_fault("TPZ5001", "`?` requires a `Result`, found `" + tpz_kind(value) + "` (§13)", span)


def tpz_spread_values(value: object, span: tuple[int, int, int]) -> list[object]:
    if not isinstance(value, list):
        tpz_fault("TPZ5001", "a spread argument must be an Array (§5)", span)
    return list(value)


def tpz_call_order_fault(
    _evaluated: list[object],
    message: str,
    span: tuple[int, int, int],
) -> object:
    tpz_fault("TPZ5004", message, span)


def tpz_nonvariadic_spread_call(
    positional: list[object],
    _spread_tail: list[object],
    _named_values: list[tuple[str, object]],
    arity: int,
    span: tuple[int, int, int],
) -> object:
    if len(positional) > arity:
        tpz_fault("TPZ5004", f"expected {arity} argument(s), found more", span)
    tpz_fault("TPZ5004", "spread arguments require a variadic parameter (§5)", span)


def tpz_nonvariadic_static_spread_call(
    _positional: list[object],
    _spread_tail: list[object],
    _named_values: list[tuple[str, object]],
    span: tuple[int, int, int],
) -> object:
    tpz_fault("TPZ5004", "spread arguments require a variadic parameter (§5)", span)


def tpz_for_items(value: object, span: tuple[int, int, int]) -> list[object]:
    if isinstance(value, list):
        return list(value)
    if isinstance(value, TpzSet):
        return list(value)
    if isinstance(value, TpzRange):
        return list(value)
    if isinstance(value, TpzMap):
        tpz_fault("TPZ5001", "`for` over `Map` is a static error; iterate `m.keys` (§10)", span)
    if isinstance(value, str):
        tpz_fault("TPZ5001", "strings are not `for`-iterable; use `s.scalars()` (§10)", span)
    tpz_fault("TPZ5001", "`" + tpz_kind(value) + "` is not `for`-iterable (§10)", span)


def tpz_in(item: object, value: object, span: tuple[int, int, int]) -> bool:
    if isinstance(value, TpzSet):
        return tpz_set_contains(value, item, span)
    if isinstance(value, TpzMap):
        tpz_fault("TPZ5001", "`x in map` is a static error; use `x in map.keys` (§9)", span)
    for candidate in tpz_for_items(value, span):
        if tpz_eq(item, candidate, span):
            return True
    return False


def tpz_for_pattern(condition: object, span: tuple[int, int, int]) -> bool:
    if condition is True:
        return True
    tpz_fault("TPZ5001", "`for` pattern did not match an element", span)


def tpz_let_pattern(condition: object, span: tuple[int, int, int]) -> bool:
    if condition is True:
        return True
    tpz_fault("TPZ5001", "`let` pattern did not match the value (§4)", span)


def tpz_call(value: object, args: tuple[object, ...], kwargs: dict[str, object], span: tuple[int, int, int]) -> object:
    if isinstance(value, TpzComposed):
        return value.call_with_span(args, kwargs, span)
    if not callable(value):
        tpz_fault("TPZ5005", "`" + tpz_kind(value) + "` is not callable", span)
    return value(*args, **kwargs)


def tpz_call_cooperative(value: object, args: tuple[object, ...], kwargs: dict[str, object], span: tuple[int, int, int]):
    cooperative_call = getattr(value, "__call_cooperative__", None)
    if cooperative_call is not None:
        return (yield from cooperative_call(*args, **kwargs))
    if not callable(value):
        tpz_fault("TPZ5005", "`" + tpz_kind(value) + "` is not callable", span)
    result = value(*args, **kwargs)
    if isinstance(result, types.GeneratorType):
        return (yield from result)
    return result


def _tpz_call_callback_co(callback: object, args: tuple[object, ...], span: tuple[int, int, int]):
    return (yield from tpz_call_cooperative(callback, args, {}, span))


def tpz_compose(first: object, second: object, span: tuple[int, int, int]) -> TpzComposed:
    return TpzComposed(first, second, span)


def tpz_host_callable(
    fn: object,
    host: object,
    cooperative_fn: object | None = None,
    variadic_py_name: str | None = None,
) -> TpzHostCallable:
    return TpzHostCallable(fn, host, cooperative_fn, variadic_py_name)


def tpz_method_dispatch_id(value: object) -> str | None:
    identity = getattr(value, "__topaz_method_identity__", None)
    if isinstance(identity, str):
        return identity
    if isinstance(value, TpzNewtype):
        return value.method_identity or value.newtype_id
    if isinstance(value, TpzEnum):
        return value.method_identity or value.enum_id
    if _is_topaz_nominal_record(value):
        return value.__topaz_record_id__
    return None


def tpz_nominal_id(value: object) -> str | None:
    if isinstance(value, TpzNewtype):
        return value.newtype_id
    if isinstance(value, TpzEnum):
        return value.enum_id
    if _is_topaz_nominal_record(value):
        return value.__topaz_record_id__
    return None


def _tpz_effective_nominal_declaration_identity(
    source_name: str, declaration_identity: object
) -> str:
    return declaration_identity if isinstance(declaration_identity, str) else source_name


def _tpz_nominal_declaration_identity(value: object) -> str | None:
    if isinstance(value, TpzNewtype):
        return _tpz_effective_nominal_declaration_identity(
            value.newtype_id, value.declaration_identity
        )
    if isinstance(value, TpzEnum):
        return _tpz_effective_nominal_declaration_identity(
            value.enum_id, value.declaration_identity
        )
    if _is_topaz_nominal_record(value):
        return _tpz_effective_nominal_declaration_identity(
            value.__topaz_record_id__,
            getattr(value, "__topaz_declaration_identity__", None),
        )
    return None


def tpz_method_registry() -> dict[tuple[str, str], object]:
    return {}


def tpz_bound_user_method(
    registry: dict[tuple[str, str], object],
    receiver: object,
    method_name: str,
    py_field: str,
    member_span: tuple[int, int, int],
) -> TpzBoundUserMethod:
    identity = tpz_method_dispatch_id(receiver)
    method = registry.get((identity, method_name)) if identity is not None else None
    if method is not None:
        return TpzBoundUserMethod(method, receiver, True)
    fallback = tpz_member(receiver, py_field, method_name, member_span)
    return TpzBoundUserMethod(fallback, receiver, False)


def _tpz_user_method_bound_args(
    value: TpzBoundUserMethod,
    pieces: list[tuple[str, str | None, object]],
    span: tuple[int, int, int],
) -> tuple[tuple[object, ...], dict[str, object]]:
    method = value.method
    if not value.prepend_receiver or not isinstance(method, TpzHostCallable):
        positional: list[object] = []
        kwargs: dict[str, object] = {}
        for kind, name, item in pieces:
            if kind == "pos":
                positional.append(item)
            elif kind == "named" and name is not None:
                kwargs[name] = item
            else:
                tpz_fault("TPZ5004", "spread arguments require a variadic parameter (§5)", span)
        return tuple(positional), kwargs

    signature = list(inspect.signature(method.fn).parameters.values())
    # Generated method functions begin with host and self. The bound wrapper
    # supplies self; call-site pieces describe only the remaining parameters.
    params = signature[2:]
    variadic_name = method.variadic_py_name
    variadic_index = next(
        (index for index, param in enumerate(params) if param.name == variadic_name),
        None,
    )
    fixed = params if variadic_index is None else params[:variadic_index]
    slots: list[tuple[str, object] | None] = [None] * len(fixed)
    tail: list[object] = []
    positional_index = 0
    saw_spread = False

    for kind, name, item in pieces:
        if kind == "pos":
            if not saw_spread and positional_index < len(fixed):
                slots[positional_index] = ("pos", item)
            else:
                tail.append(item)
            positional_index += 1
            continue
        if kind == "spread":
            if variadic_index is None:
                tpz_fault("TPZ5004", "spread arguments require a variadic parameter (§5)", span)
            if any(
                slots[index] is None and param.default is inspect.Parameter.empty
                for index, param in enumerate(fixed[positional_index:], positional_index)
            ):
                tpz_fault(
                    "TPZ5004",
                    "a spread argument cannot skip an unsatisfied fixed parameter (§5)",
                    span,
                )
            tail.extend(item)
            saw_spread = True
            continue
        if kind == "named" and name is not None:
            index = next((i for i, param in enumerate(fixed) if param.name == name), None)
            if index is None or slots[index] is not None:
                tpz_fault("TPZ5004", "invalid named argument", span)
            slots[index] = ("named", item)

    positional: list[object] = []
    kwargs: dict[str, object] = {}
    for param, slot in zip(fixed, slots):
        if slot is None:
            if param.default is inspect.Parameter.empty:
                tpz_fault("TPZ5004", "missing required argument", span)
            continue
        mode, item = slot
        if mode == "pos":
            positional.append(item)
        else:
            kwargs[param.name] = item
    if variadic_name is not None:
        kwargs[variadic_name] = tail
    return tuple(positional), kwargs


def tpz_user_method_call(
    value: TpzBoundUserMethod,
    pieces: list[tuple[str, str | None, object]],
    span: tuple[int, int, int],
) -> object:
    args, kwargs = _tpz_user_method_bound_args(value, pieces, span)
    return tpz_call(value, args, kwargs, span)


def tpz_user_method_call_cooperative(
    value: TpzBoundUserMethod,
    pieces: list[tuple[str, str | None, object]],
    span: tuple[int, int, int],
):
    args, kwargs = _tpz_user_method_bound_args(value, pieces, span)
    return (yield from tpz_call_cooperative(value, args, kwargs, span))


def _tpz_builtin_protocol_dispatch(
    protocol: str,
    method_name: str,
    args: list[object],
    span: tuple[int, int, int],
) -> object:
    if protocol == "Show" and method_name == "show":
        value = args[0] if args else TPZ_UNIT
        return tpz_render(value)
    if protocol == "Eq" and method_name == "equals":
        left = args[0] if args else TPZ_UNIT
        right = args[1] if len(args) > 1 else TPZ_UNIT
        return _tpz_values_equal(left, right, span)
    if protocol == "Order" and method_name == "compare":
        left = args[0] if args else TPZ_UNIT
        right = args[1] if len(args) > 1 else TPZ_UNIT
        return _tpz_order_compare(left, right, span)
    tpz_fault(
        "TPZ5001",
        f"no derived protocol method `{protocol}.{method_name}`",
        span,
    )


def _tpz_protocol_method(
    registry: dict[tuple[str, str], object],
    module: str,
    protocol: str,
    method_name: str,
    args: list[object],
) -> object | None:
    nominal_id = tpz_nominal_id(args[0]) if args else None
    if nominal_id is None:
        return None
    return registry.get((f"{module}::{protocol}<{nominal_id}>", method_name))


def tpz_protocol_call(
    registry: dict[tuple[str, str], object],
    module: str,
    protocol: str,
    method_name: str,
    args: list[object],
    span: tuple[int, int, int],
) -> object:
    manual = _tpz_protocol_method(registry, module, protocol, method_name, args)
    if manual is not None:
        return tpz_call(manual, tuple(args), {}, span)
    return _tpz_builtin_protocol_dispatch(protocol, method_name, args, span)


def tpz_protocol_call_cooperative(
    registry: dict[tuple[str, str], object],
    module: str,
    protocol: str,
    method_name: str,
    args: list[object],
    span: tuple[int, int, int],
):
    manual = _tpz_protocol_method(registry, module, protocol, method_name, args)
    if manual is not None:
        return (yield from tpz_call_cooperative(manual, tuple(args), {}, span))
    return _tpz_builtin_protocol_dispatch(protocol, method_name, args, span)


def tpz_cooperative_callable(fn: object, cooperative_fn: object) -> TpzCooperativeCallable:
    return TpzCooperativeCallable(fn, cooperative_fn)


def tpz_extern_function(module: str, function: str, span: tuple[int, int, int]) -> TpzExternFunction:
    return TpzExternFunction(module, function, span)


def tpz_run_defer(host: Host, thunk) -> None:
    try:
        value = thunk()
    except TpzFault as fault:
        host.defer_error(fault.code + ": " + fault.message, fault)
        return
    except TpzReturn as returned:
        host.defer_error(tpz_render(returned.value))
        return
    if isinstance(value, Err):
        host.defer_error(tpz_render(value.value))


def tpz_file_read(value: object, member_span: tuple[int, int, int]) -> Ok | Err:
    if not isinstance(value, TpzFile):
        tpz_no_member(value, "read", member_span)
    return value.host.read_file(value.handle)


def tpz_file_write(
    value: object,
    text: object,
    member_span: tuple[int, int, int],
    call_span: tuple[int, int, int],
) -> Ok | Err:
    if not isinstance(value, TpzFile):
        tpz_no_member(value, "write", member_span)
    return value.host.write_file(value.handle, text, call_span)


def tpz_file_close(value: object, member_span: tuple[int, int, int]) -> object:
    if not isinstance(value, TpzFile):
        tpz_no_member(value, "close", member_span)
    value.host.close_file(value.handle)
    return TPZ_UNIT


def tpz_using_file(value: object, span: tuple[int, int, int]) -> TpzFile:
    if not isinstance(value, TpzFile):
        tpz_fault(
            "TPZ5001",
            "`using` expects a `File`, found `" + tpz_kind(value) + "`",
            span,
        )
    return value


def _fs_path_arg(value: object, method: str, span: tuple[int, int, int]) -> str:
    if isinstance(value, str):
        return value
    tpz_fault(
        "TPZ5001",
        "`FS." + method + "` parameter `path` expects string or Path; found `" + tpz_kind(value) + "`",
        span,
    )


def _fs_bytes_arg(value: object, method: str, span: tuple[int, int, int]) -> TpzBytes:
    if isinstance(value, TpzBytes):
        return value
    tpz_fault(
        "TPZ5001",
        "`FS." + method + "` takes `Bytes`, found `" + tpz_kind(value) + "`",
        span,
    )


def _fs_dir_entry(name: str, kind: str, size_bytes: int | None) -> object:
    fields = (
        ("_t_6b696e64", "kind"),
        ("_t_6e616d65", "name"),
        ("_t_73697a654279746573", "sizeBytes"),
    )
    cls = _record_class_for_fields(fields)
    return cls(
        _t_6b696e64=kind,
        _t_6e616d65=name,
        _t_73697a654279746573=Some(size_bytes) if size_bytes is not None else None,
    )


def tpz_fs_read_text(host: Host, path: object, span: tuple[int, int, int]) -> Ok | Err:
    return host.fs_read_text(_fs_path_arg(path, "readText", span))


def tpz_fs_write_text(
    host: Host,
    path: object,
    text: object,
    span: tuple[int, int, int],
) -> Ok | Err:
    path = _fs_path_arg(path, "writeText", span)
    if not isinstance(text, str):
        tpz_fault(
            "TPZ5001",
            "`FS.writeText` takes `text: string`, found `" + tpz_kind(text) + "`",
            span,
        )
    return host.fs_write_text(path, text)


def tpz_fs_read_bytes(host: Host, path: object, span: tuple[int, int, int]) -> Ok | Err:
    return host.fs_read_bytes(_fs_path_arg(path, "readBytes", span))


def tpz_fs_write_bytes(
    host: Host,
    path: object,
    bytes_value: object,
    span: tuple[int, int, int],
) -> Ok | Err:
    path = _fs_path_arg(path, "writeBytes", span)
    return host.fs_write_bytes(path, _fs_bytes_arg(bytes_value, "writeBytes", span))


def tpz_fs_list(host: Host, path: object, span: tuple[int, int, int]) -> Ok | Err:
    return host.fs_list(_fs_path_arg(path, "list", span))


def tpz_no_member(value: object, member: str, span: tuple[int, int, int]) -> None:
    tpz_fault(
        "TPZ5006",
        "`" + tpz_kind(value) + "` has no member `" + member + "`",
        span,
    )


_HEX = "0123456789abcdef"
_BASE64_ALPHABET = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/"


def _bytes_receiver(value: object, name: str, span: tuple[int, int, int]) -> TpzBytes:
    if not isinstance(value, TpzBytes):
        tpz_fault(
            "TPZ5001",
            "`Bytes." + name + "` takes a `Bytes`, found `" + tpz_kind(value) + "`",
            span,
        )
    return value


def _bytes_str_arg(value: object, name: str, span: tuple[int, int, int]) -> str:
    if not isinstance(value, str):
        tpz_fault(
            "TPZ5001",
            "`Bytes." + name + "` takes a `string`, found `" + tpz_kind(value) + "`",
            span,
        )
    return value


def _hex_nibble(ch: str) -> int | None:
    code = ord(ch)
    if 48 <= code <= 57:
        return code - 48
    if 97 <= code <= 102:
        return code - 87
    if 65 <= code <= 70:
        return code - 55
    return None


def tpz_bytes_to_hex(value: object, span: tuple[int, int, int] | None = None) -> str:
    raw = _bytes_receiver(value, "toHex", span or (0, 0, 0)).data
    out: list[str] = []
    for byte in raw:
        out.append(_HEX[byte >> 4])
        out.append(_HEX[byte & 0x0F])
    return "".join(out)


def _bytes_to_base64(raw: bytes) -> str:
    out: list[str] = []
    for i in range(0, len(raw), 3):
        chunk = raw[i : i + 3]
        b0 = chunk[0]
        b1 = chunk[1] if len(chunk) > 1 else 0
        b2 = chunk[2] if len(chunk) > 2 else 0
        n = (b0 << 16) | (b1 << 8) | b2
        out.append(_BASE64_ALPHABET[(n >> 18) & 0x3F])
        out.append(_BASE64_ALPHABET[(n >> 12) & 0x3F])
        out.append(_BASE64_ALPHABET[(n >> 6) & 0x3F] if len(chunk) > 1 else "=")
        out.append(_BASE64_ALPHABET[n & 0x3F] if len(chunk) > 2 else "=")
    return "".join(out)


def _base64_sextet(ch: str) -> int | None:
    code = ord(ch)
    if 65 <= code <= 90:
        return code - 65
    if 97 <= code <= 122:
        return code - 71
    if 48 <= code <= 57:
        return code + 4
    if ch == "+":
        return 62
    if ch == "/":
        return 63
    return None


def tpz_bytes_empty() -> TpzBytes:
    return TpzBytes(b"")


def tpz_bytes_encode_utf8(value: object, span: tuple[int, int, int]) -> TpzBytes:
    return TpzBytes(_bytes_str_arg(value, "encodeUtf8", span).encode("utf-8"))


def tpz_bytes_from_array(value: object, span: tuple[int, int, int]) -> Ok | Err:
    if not isinstance(value, list):
        tpz_fault(
            "TPZ5001",
            "`Bytes.fromArray` takes an `Array<int>`, found `" + tpz_kind(value) + "`",
            span,
        )
    out = bytearray()
    for idx, item in enumerate(value):
        if type(item) is int and 0 <= item <= 255:
            out.append(item)
        elif type(item) is int:
            return Err("Bytes.fromArray: value at index " + str(idx) + " is outside 0..255")
        else:
            return Err(
                "Bytes.fromArray: value at index "
                + str(idx)
                + " is `"
                + tpz_kind(item)
                + "`, expected `int`"
            )
    return Ok(TpzBytes(bytes(out)))


def tpz_bytes_from_hex(value: object, span: tuple[int, int, int]) -> Ok | Err:
    text = _bytes_str_arg(value, "fromHex", span)
    if len(text) % 2 != 0:
        return Err("Bytes.fromHex: odd-length hex string")
    out = bytearray()
    for i in range(0, len(text), 2):
        hi = _hex_nibble(text[i])
        lo = _hex_nibble(text[i + 1])
        if hi is None or lo is None:
            return Err("Bytes.fromHex: invalid hex digit")
        out.append((hi << 4) | lo)
    return Ok(TpzBytes(bytes(out)))


def tpz_bytes_to_base64(value: object, span: tuple[int, int, int]) -> str:
    return _bytes_to_base64(_bytes_receiver(value, "toBase64", span).data)


def tpz_bytes_from_base64(value: object, span: tuple[int, int, int]) -> Ok | Err:
    text = _bytes_str_arg(value, "fromBase64", span)
    if text == "":
        return Ok(TpzBytes(b""))
    if len(text) % 4 != 0:
        return Err("Bytes.fromBase64: length is not a multiple of 4")
    pad = text.count("=")
    if pad > 2 or "=" in text[: len(text) - pad]:
        return Err("Bytes.fromBase64: misplaced padding")
    out = bytearray()
    for i in range(0, len(text), 4):
        group = text[i : i + 4]
        pads = group.count("=")
        sextets = [0, 0, 0, 0]
        for j, ch in enumerate(group):
            if ch == "=":
                continue
            value = _base64_sextet(ch)
            if value is None:
                return Err("Bytes.fromBase64: invalid base64 character")
            sextets[j] = value
        n = (sextets[0] << 18) | (sextets[1] << 12) | (sextets[2] << 6) | sextets[3]
        if pads == 0:
            out.append((n >> 16) & 0xFF)
            out.append((n >> 8) & 0xFF)
            out.append(n & 0xFF)
        elif pads == 1:
            if n & 0xFF:
                return Err("Bytes.fromBase64: non-canonical padding bits")
            out.append((n >> 16) & 0xFF)
            out.append((n >> 8) & 0xFF)
        elif pads == 2:
            if n & 0xFFFF:
                return Err("Bytes.fromBase64: non-canonical padding bits")
            out.append((n >> 16) & 0xFF)
        else:
            return Err("Bytes.fromBase64: misplaced padding")
    return Ok(TpzBytes(bytes(out)))


def tpz_bytes_decode_utf8(value: object, span: tuple[int, int, int]) -> Ok | Err:
    raw = _bytes_receiver(value, "decodeUtf8", span).data
    try:
        return Ok(raw.decode("utf-8"))
    except UnicodeDecodeError:
        return Err("Bytes.decodeUtf8: invalid UTF-8")


def tpz_bytes_length(value: object, span: tuple[int, int, int]) -> int:
    return len(_bytes_receiver(value, "length", span).data)


def tpz_bytes_is_empty(value: object, span: tuple[int, int, int]) -> bool:
    return len(_bytes_receiver(value, "isEmpty", span).data) == 0


def tpz_bytes_get(value: object, index: object, span: tuple[int, int, int]) -> Some | None:
    raw = _bytes_receiver(value, "get", span).data
    if type(index) is not int:
        tpz_fault("TPZ5001", "`Bytes.get` takes an `int` index", span)
    if 0 <= index < len(raw):
        return Some(raw[index])
    return None


def tpz_bytes_slice(value: object, start: object, end: object, span: tuple[int, int, int]) -> TpzBytes:
    raw = _bytes_receiver(value, "slice", span).data
    if type(start) is not int:
        tpz_fault(
            "TPZ5001",
            "`Bytes.slice` takes `int` bounds, found `" + tpz_kind(start) + "`",
            span,
        )
    if type(end) is not int:
        tpz_fault(
            "TPZ5001",
            "`Bytes.slice` takes `int` bounds, found `" + tpz_kind(end) + "`",
            span,
        )
    length = len(raw)
    s = max(0, min(start, length))
    e = max(s, min(end, length))
    return TpzBytes(raw[s:e])


def tpz_bytes_to_array(value: object, span: tuple[int, int, int]) -> list[int]:
    return list(_bytes_receiver(value, "toArray", span).data)


def tpz_bytes_concat(a: object, b: object, span: tuple[int, int, int]) -> TpzBytes:
    left = _bytes_receiver(a, "concat", span).data
    right = _bytes_receiver(b, "concat", span).data
    return TpzBytes(left + right)


def _byte_buffer_receiver(
    value: object, name: str, span: tuple[int, int, int]
) -> TpzByteBuffer:
    if not isinstance(value, TpzByteBuffer):
        tpz_fault(
            "TPZ5001",
            "`ByteBuffer." + name + "` requires `ByteBuffer`, found `" + tpz_kind(value) + "`",
            span,
        )
    return value


def _byte_buffer_int(value: object, label: str, span: tuple[int, int, int]) -> int:
    if type(value) is not int:
        tpz_fault(
            "TPZ5001",
            "`ByteBuffer` " + label + " must be `int`, found `" + tpz_kind(value) + "`",
            span,
        )
    return value


def _byte_buffer_index(value: object, label: str, span: tuple[int, int, int]) -> int:
    value = _byte_buffer_int(value, label, span)
    if value < 0:
        tpz_fault("TPZ4001", "`ByteBuffer` " + label + " must be non-negative", span)
    return value


def _byte_buffer_byte(value: object, span: tuple[int, int, int]) -> int:
    value = _byte_buffer_int(value, "byte value", span)
    if value < 0 or value > 255:
        tpz_fault("TPZ5001", "`ByteBuffer` byte value must be in 0..255", span)
    return value


def _byte_buffer_range(
    start: object, length: object, size: int, span: tuple[int, int, int]
) -> tuple[int, int]:
    start = _byte_buffer_index(start, "start", span)
    length = _byte_buffer_index(length, "length", span)
    if start > size or length > size - start:
        tpz_fault("TPZ4001", "`ByteBuffer` range is out of bounds", span)
    return start, start + length


def tpz_byte_buffer_allocate(
    length: object, value: object = 0, span: tuple[int, int, int] = (0, 0, 0)
) -> TpzByteBuffer:
    length = _byte_buffer_index(length, "length", span)
    value = _byte_buffer_byte(value, span)
    try:
        return TpzByteBuffer(bytearray([value]) * length)
    except (MemoryError, OverflowError):
        tpz_fault("TPZ5001", "`ByteBuffer.allocate` length cannot be allocated", span)


def tpz_byte_buffer_from_bytes(value: object, span: tuple[int, int, int]) -> TpzByteBuffer:
    return TpzByteBuffer(bytearray(_bytes_receiver(value, "fromBytes", span).data))


def tpz_byte_buffer_length(value: object, span: tuple[int, int, int]) -> int:
    return len(_byte_buffer_receiver(value, "length", span).data)


def tpz_byte_buffer_get(value: object, index: object, span: tuple[int, int, int]) -> int:
    data = _byte_buffer_receiver(value, "get", span).data
    index = _byte_buffer_index(index, "index", span)
    if index >= len(data):
        tpz_fault("TPZ4001", "`ByteBuffer.get` index is out of bounds", span)
    return data[index]


def tpz_byte_buffer_set(
    value: object, index: object, byte: object, span: tuple[int, int, int]
) -> object:
    data = _byte_buffer_receiver(value, "set", span).data
    index = _byte_buffer_index(index, "index", span)
    byte = _byte_buffer_byte(byte, span)
    if index >= len(data):
        tpz_fault("TPZ4001", "`ByteBuffer.set` index is out of bounds", span)
    data[index] = byte
    return TPZ_UNIT


def tpz_byte_buffer_fill(
    value: object, start: object, length: object, byte: object, span: tuple[int, int, int]
) -> object:
    data = _byte_buffer_receiver(value, "fill", span).data
    begin, end = _byte_buffer_range(start, length, len(data), span)
    byte = _byte_buffer_byte(byte, span)
    data[begin:end] = bytes([byte]) * (end - begin)
    return TPZ_UNIT


def tpz_byte_buffer_copy(
    target: object,
    source: object,
    source_start: object,
    target_start: object,
    length: object,
    span: tuple[int, int, int],
) -> object:
    target_data = _byte_buffer_receiver(target, "copy", span).data
    source_data = _byte_buffer_receiver(source, "copy", span).data
    count = _byte_buffer_index(length, "length", span)
    source_start = _byte_buffer_index(source_start, "source start", span)
    target_start = _byte_buffer_index(target_start, "target start", span)
    if (
        source_start > len(source_data)
        or count > len(source_data) - source_start
        or target_start > len(target_data)
        or count > len(target_data) - target_start
    ):
        tpz_fault("TPZ4001", "`ByteBuffer.copy` range is out of bounds", span)
    snapshot = bytes(source_data[source_start : source_start + count])
    target_data[target_start : target_start + count] = snapshot
    return TPZ_UNIT


def tpz_byte_buffer_to_bytes(value: object, span: tuple[int, int, int]) -> TpzBytes:
    return TpzBytes(bytes(_byte_buffer_receiver(value, "toBytes", span).data))


def _key(
    value: object,
    span: tuple[int, int, int],
    budget: _StructBudget | None = None,
    depth: int = 0,
) -> object:
    if budget is None:
        budget = _StructBudget()
    budget.consume(depth, span)
    if type(value) is bool:
        return ("bool", value)
    if type(value) is int:
        return ("int", value)
    if type(value) is float:
        return ("f64", TpzFloatKey(tpz_f64_bits(value), value))
    if isinstance(value, str):
        return ("str", value)
    if value is TPZ_UNIT:
        return ("unit", None)
    if value is TPZ_NULL:
        return ("null", None)
    if value is None:
        return ("none", None)
    if isinstance(value, TpzBytes):
        return ("bytes", value.data)
    if isinstance(value, TpzByteBuffer):
        tpz_fault("TPZ5007", "`ByteBuffer` values are not comparable", span)
    if isinstance(value, Some):
        return ("some", _key(value.value, span, budget, depth + 1))
    if isinstance(value, Ok):
        return ("ok", _key(value.value, span, budget, depth + 1))
    if isinstance(value, Err):
        return ("err", _key(value.value, span, budget, depth + 1))
    if isinstance(value, list):
        return ("list", tuple(_key(item, span, budget, depth + 1) for item in value))
    if _is_topaz_newtype(value):
        if value.declaration_identity is not None:
            return (
                "newtype_decl",
                value.declaration_identity,
                value.newtype_id,
                _key(value.value, span, budget, depth + 1),
            )
        return ("newtype", value.newtype_id, _key(value.value, span, budget, depth + 1))
    if _is_topaz_enum(value):
        if value.enum_id == "RoundingMode":
            tpz_fault("TPZ5007", "`RoundingMode` values are not comparable", span)
        if value.declaration_identity is not None:
            return (
                "enum_decl",
                value.declaration_identity,
                value.enum_id,
                value.variant,
                value.variant_index,
                tuple(_key(item, span, budget, depth + 1) for item in value.payloads),
            )
        return (
            "enum",
            value.enum_id,
            value.variant,
            value.variant_index,
            tuple(_key(item, span, budget, depth + 1) for item in value.payloads),
        )
    if isinstance(value, TpzMap):
        tpz_fault("TPZ5007", "`Map` values are not comparable", span)
    if isinstance(value, TpzSet):
        tpz_fault("TPZ5007", "`Set` values are not comparable", span)
    if isinstance(value, TpzFile):
        tpz_fault("TPZ5007", "`File` values are not comparable", span)
    if isinstance(value, TpzJson):
        tpz_fault("TPZ5007", "`JSONValue` values are not comparable", span)
    if _is_topaz_nominal_record(value):
        fields = []
        for py_field, source_field in value.__topaz_record_fields__:
            fields.append(
                (py_field, source_field, _key(getattr(value, py_field), span, budget, depth + 1))
            )
        declaration_identity = getattr(value, "__topaz_declaration_identity__", None)
        if isinstance(declaration_identity, str):
            return (
                "nominal_record_decl",
                declaration_identity,
                value.__topaz_record_id__,
                tuple(fields),
            )
        return ("nominal_record", value.__topaz_record_id__, tuple(fields))
    if _is_topaz_record(value):
        fields = []
        for py_field, source_field in sorted(
            value.__topaz_record_fields__, key=lambda item: item[1]
        ):
            fields.append(
                (py_field, source_field, _key(getattr(value, py_field), span, budget, depth + 1))
            )
        return ("record", tuple(fields))
    tpz_fault("TPZ5007", "`" + tpz_kind(value) + "` values are not comparable", span)


def _key_to_value(key: object) -> object:
    tag = key[0]
    value = key[1]
    if tag in ("bool", "int", "f64", "str"):
        if tag == "f64":
            if isinstance(value, TpzFloatKey):
                return tpz_f64_from_bits(value.bits)
            return tpz_f64_from_bits(value)
        return value
    if tag == "unit":
        return TPZ_UNIT
    if tag == "null":
        return TPZ_NULL
    if tag == "none":
        return None
    if tag == "bytes":
        return TpzBytes(value)
    if tag == "some":
        return Some(_key_to_value(value))
    if tag == "ok":
        return Ok(_key_to_value(value))
    if tag == "err":
        return Err(_key_to_value(value))
    if tag == "list":
        return [_key_to_value(item) for item in value]
    if tag == "newtype":
        return TpzNewtype(key[1], _key_to_value(key[2]))
    if tag == "newtype_decl":
        return TpzNewtype(key[2], _key_to_value(key[3]), None, key[1])
    if tag == "enum":
        return TpzEnum(key[1], key[2], key[3], tuple(_key_to_value(item) for item in key[4]))
    if tag == "enum_decl":
        return TpzEnum(
            key[2],
            key[3],
            key[4],
            tuple(_key_to_value(item) for item in key[5]),
            None,
            key[1],
        )
    if tag == "record":
        metadata = tuple((py_field, source_field) for py_field, source_field, _ in value)
        values = {
            py_field: _key_to_value(item) for py_field, _source_field, item in value
        }
        return _record_class_for_fields(metadata)(**values)
    if tag == "nominal_record":
        record_id = key[1]
        fields = key[2]
        metadata = tuple((py_field, source_field) for py_field, source_field, _ in fields)
        values = {
            py_field: _key_to_value(item) for py_field, _source_field, item in fields
        }
        return _record_class_for_fields(metadata, record_id)(**values)
    if tag == "nominal_record_decl":
        declaration_identity = key[1]
        record_id = key[2]
        fields = key[3]
        metadata = tuple((py_field, source_field) for py_field, source_field, _ in fields)
        values = {
            py_field: _key_to_value(item) for py_field, _source_field, item in fields
        }
        return _record_class_for_fields(metadata, record_id, declaration_identity)(**values)
    raise TypeError("unknown Topaz key tag")


def _map_receiver(value: object, name: str, span: tuple[int, int, int]) -> TpzMap:
    if not isinstance(value, TpzMap):
        tpz_fault("TPZ5001", "`Map." + name + "` takes a `Map`, found `" + tpz_kind(value) + "`", span)
    return value


def _set_receiver(value: object, name: str, span: tuple[int, int, int]) -> TpzSet:
    if not isinstance(value, TpzSet):
        tpz_fault("TPZ5001", "`Set." + name + "` takes a `Set`, found `" + tpz_kind(value) + "`", span)
    return value


def _map_find(entries: list[tuple[object, object]], key: object) -> int | None:
    for idx, (existing, _) in enumerate(entries):
        if existing == key:
            return idx
    return None


def tpz_map_new() -> TpzMap:
    return TpzMap([])


def tpz_map_of(pairs: list[tuple[object, object]], span: tuple[int, int, int]) -> TpzMap:
    out = TpzMap([])
    for key_value, value in pairs:
        key = _key(key_value, span)
        if _map_find(out.entries, key) is not None:
            tpz_fault("TPZ4601", "duplicate key in `map { … }` literal", span)
        out.entries.append((key, value))
    return out


_MAP_ENTRY_RECORD_FIELDS = (("_t_6b6579", "key"), ("_t_76616c7565", "value"))


def _map_entry_record(key: object, value: object) -> object:
    cls = _record_class_for_fields(_MAP_ENTRY_RECORD_FIELDS)
    return cls(_t_6b6579=key, _t_76616c7565=value)


def _map_entry_fields(entry: object, span: tuple[int, int, int]) -> tuple[object, object]:
    if not _is_topaz_record(entry) or _is_topaz_nominal_record(entry):
        tpz_fault(
            "TPZ5001",
            "`Map.ofEntries` entries must be records `{ key, value }`, found `" + tpz_kind(entry) + "`",
            span,
        )
    metadata = getattr(entry, "__topaz_record_fields__", ())
    fields_by_source = {source_field: py_field for py_field, source_field in metadata}
    if "key" not in fields_by_source or "value" not in fields_by_source:
        tpz_fault("TPZ5001", "`Map.ofEntries` entries must have `key` and `value` fields", span)
    if len(metadata) != 2:
        tpz_fault("TPZ5001", "`Map.ofEntries` entries must have exactly `key` and `value` fields", span)
    return getattr(entry, fields_by_source["key"]), getattr(entry, fields_by_source["value"])


def tpz_map_of_entries(entries: object, span: tuple[int, int, int]) -> TpzMap:
    if not isinstance(entries, list):
        tpz_fault(
            "TPZ5001",
            "`Map.ofEntries` takes an `Array` of `{ key, value }` records, found `" + tpz_kind(entries) + "`",
            span,
        )
    out = TpzMap([])
    for entry in entries:
        key_value, value = _map_entry_fields(entry, span)
        key = _key(key_value, span)
        idx = _map_find(out.entries, key)
        if idx is None:
            out.entries.append((key, value))
        else:
            out.entries[idx] = (out.entries[idx][0], value)
    return out


def tpz_map_insert(value: object, key_value: object, item: object, span: tuple[int, int, int]) -> object:
    entries = _map_receiver(value, "insert", span).entries
    key = _key(key_value, span)
    idx = _map_find(entries, key)
    if idx is None:
        entries.append((key, item))
    else:
        entries[idx] = (entries[idx][0], item)
    return TPZ_UNIT


def tpz_map_get(value: object, key_value: object, span: tuple[int, int, int]) -> Some | None:
    entries = _map_receiver(value, "get", span).entries
    key = _key(key_value, span)
    idx = _map_find(entries, key)
    return Some(entries[idx][1]) if idx is not None else None


def tpz_map_get_or(value: object, key_value: object, default: object, span: tuple[int, int, int]) -> object:
    found = tpz_map_get(value, key_value, span)
    return found.value if isinstance(found, Some) else default


def tpz_map_contains_key(value: object, key_value: object, span: tuple[int, int, int]) -> bool:
    entries = _map_receiver(value, "containsKey", span).entries
    return _map_find(entries, _key(key_value, span)) is not None


def tpz_map_is_empty(value: object, span: tuple[int, int, int]) -> bool:
    return len(_map_receiver(value, "isEmpty", span).entries) == 0


def tpz_map_remove(value: object, key_value: object, span: tuple[int, int, int]) -> Some | None:
    entries = _map_receiver(value, "remove", span).entries
    idx = _map_find(entries, _key(key_value, span))
    if idx is None:
        return None
    _, removed = entries.pop(idx)
    return Some(removed)


def tpz_map_clear(value: object, span: tuple[int, int, int]) -> object:
    _map_receiver(value, "clear", span).entries.clear()
    return TPZ_UNIT


def tpz_map_map_values(value: object, callback: object, span: tuple[int, int, int]) -> TpzMap:
    entries = _map_receiver(value, "mapValues", span).entries
    if not callable(callback):
        tpz_fault("TPZ5001", "`Map.mapValues` callback must be callable", span)
    out = TpzMap([])
    for key, item in list(entries):
        out.entries.append((key, callback(item)))
    return out


def tpz_map_map_values__co(value: object, callback: object, span: tuple[int, int, int]) -> object:
    entries = _map_receiver(value, "mapValues", span).entries
    if not callable(callback):
        tpz_fault("TPZ5001", "`Map.mapValues` callback must be callable", span)
    out = TpzMap([])
    for key, item in list(entries):
        out.entries.append((key, (yield from _tpz_call_callback_co(callback, (item,), span))))
    return out


def tpz_map_filter(value: object, callback: object, span: tuple[int, int, int]) -> TpzMap:
    entries = _map_receiver(value, "filter", span).entries
    if not callable(callback):
        tpz_fault("TPZ5001", "`Map.filter` callback must be callable", span)
    out = TpzMap([])
    for key, item in list(entries):
        if tpz_condition(callback(_key_to_value(key), item), span):
            out.entries.append((key, item))
    return out


def tpz_map_filter__co(value: object, callback: object, span: tuple[int, int, int]) -> object:
    entries = _map_receiver(value, "filter", span).entries
    if not callable(callback):
        tpz_fault("TPZ5001", "`Map.filter` callback must be callable", span)
    out = TpzMap([])
    for key, item in list(entries):
        if tpz_condition((yield from _tpz_call_callback_co(callback, (_key_to_value(key), item), span)), span):
            out.entries.append((key, item))
    return out


def tpz_map_update(
    value: object,
    key_value: object,
    initial: object,
    callback: object,
    span: tuple[int, int, int],
) -> object:
    entries = _map_receiver(value, "update", span).entries
    key = _key(key_value, span)
    idx = _map_find(entries, key)
    if idx is None:
        entries.append((key, initial))
        return TPZ_UNIT
    if not callable(callback):
        tpz_fault("TPZ5001", "`Map.update` callback must be callable", span)
    entries[idx] = (entries[idx][0], callback(entries[idx][1]))
    return TPZ_UNIT


def tpz_map_update__co(
    value: object,
    key_value: object,
    initial: object,
    callback: object,
    span: tuple[int, int, int],
) -> object:
    entries = _map_receiver(value, "update", span).entries
    key = _key(key_value, span)
    idx = _map_find(entries, key)
    if idx is None:
        entries.append((key, initial))
        return TPZ_UNIT
    if not callable(callback):
        tpz_fault("TPZ5001", "`Map.update` callback must be callable", span)
    entries[idx] = (entries[idx][0], (yield from _tpz_call_callback_co(callback, (entries[idx][1],), span)))
    return TPZ_UNIT


def tpz_set_of(values: list[object], span: tuple[int, int, int]) -> TpzSet:
    out = TpzSet([])
    for value in values:
        key = _key(value, span)
        if key not in out.items:
            out.items.append(key)
    return out


def tpz_set_add(value: object, item: object, span: tuple[int, int, int]) -> object:
    items = _set_receiver(value, "add", span).items
    key = _key(item, span)
    if key not in items:
        items.append(key)
    return TPZ_UNIT


def tpz_set_remove(value: object, item: object, span: tuple[int, int, int]) -> bool:
    items = _set_receiver(value, "remove", span).items
    key = _key(item, span)
    if key not in items:
        return False
    items.remove(key)
    return True


def tpz_set_contains(value: object, item: object, span: tuple[int, int, int]) -> bool:
    return _key(item, span) in _set_receiver(value, "contains", span).items


def tpz_set_is_empty(value: object, span: tuple[int, int, int]) -> bool:
    return len(_set_receiver(value, "isEmpty", span).items) == 0


def tpz_set_to_array(value: object, span: tuple[int, int, int]) -> list[object]:
    return [_key_to_value(key) for key in _set_receiver(value, "toArray", span).items]


def tpz_set_union(value: object, other: object, span: tuple[int, int, int]) -> TpzSet:
    left = _set_receiver(value, "union", span).items
    right = _set_receiver(other, "union", span).items
    out = TpzSet(list(left))
    for key in right:
        if key not in out.items:
            out.items.append(key)
    return out


def tpz_set_intersection(value: object, other: object, span: tuple[int, int, int]) -> TpzSet:
    left = _set_receiver(value, "intersection", span).items
    right = _set_receiver(other, "intersection", span).items
    return TpzSet([key for key in left if key in right])


def tpz_set_difference(value: object, other: object, span: tuple[int, int, int]) -> TpzSet:
    left = _set_receiver(value, "difference", span).items
    right = _set_receiver(other, "difference", span).items
    return TpzSet([key for key in left if key not in right])


def tpz_is_empty(value: object, span: tuple[int, int, int]) -> bool:
    if isinstance(value, TpzBytes):
        return tpz_bytes_is_empty(value, span)
    if isinstance(value, TpzMap):
        return tpz_map_is_empty(value, span)
    if isinstance(value, TpzSet):
        return tpz_set_is_empty(value, span)
    tpz_no_member(value, "isEmpty", span)


def tpz_to_array(value: object, span: tuple[int, int, int]) -> list[object]:
    if isinstance(value, TpzBytes):
        return tpz_bytes_to_array(value, span)
    if isinstance(value, TpzSet):
        return tpz_set_to_array(value, span)
    tpz_no_member(value, "toArray", span)


def tpz_remove(value: object, item: object, span: tuple[int, int, int]) -> object:
    if isinstance(value, TpzMap):
        return tpz_map_remove(value, item, span)
    if isinstance(value, TpzSet):
        return tpz_set_remove(value, item, span)
    tpz_no_member(value, "remove", span)


def tpz_clear(value: object, span: tuple[int, int, int]) -> object:
    if isinstance(value, list):
        return tpz_array_clear(value, span)
    if isinstance(value, TpzMap):
        return tpz_map_clear(value, span)
    if isinstance(value, TpzSet):
        value.items.clear()
        return TPZ_UNIT
    tpz_no_member(value, "clear", span)


def tpz_member(value: object, py_field: object, source_field: object, span: tuple[int, int, int]) -> object:
    if not isinstance(source_field, str):
        tpz_fault("TPZ5001", "record field metadata is malformed", span)
    if isinstance(value, list) and source_field == "length":
        return len(value)
    if isinstance(value, TpzMap):
        if source_field == "keys":
            return [_key_to_value(key) for key, _ in value.entries]
        if source_field == "values":
            return [item for _, item in value.entries]
        if source_field == "entries":
            return [_map_entry_record(_key_to_value(key), item) for key, item in value.entries]
        if source_field == "length":
            return len(value.entries)
    if isinstance(value, TpzSet) and source_field == "length":
        return len(value.items)
    if isinstance(value, TpzNewtype):
        if source_field == "value":
            return lambda: tpz_newtype_unwrap(value, span)
        tpz_no_member(value, source_field, span)
    if isinstance(value, TpzTemplate):
        if source_field == "tag":
            return value.tag
        if source_field == "parts":
            return list(value.parts)
        # §16 the ONLY template members are `tag` and `parts`; never fall
        # through to the getattr-based record path, which would expose the
        # interpolated values the sql/sh injection-safety rules hide.
        tpz_no_member(value, source_field, span)
    return tpz_record_field(value, py_field, source_field, span)


def _wrap_optional(value: object) -> object:
    if isinstance(value, Some) or value is None:
        return value
    return Some(value)


def tpz_wrap_optional(value: object) -> object:
    return _wrap_optional(value)


def tpz_wrap_optional_unit(_value: object) -> object:
    return Some(TPZ_UNIT)


def tpz_optional_member(
    value: object,
    py_field: object,
    source_field: object,
    span: tuple[int, int, int],
) -> object:
    if value is None:
        return None
    if isinstance(value, Some):
        return _wrap_optional(tpz_member(value.value, py_field, source_field, span))
    return tpz_member(value, py_field, source_field, span)


class _JsonParseError(Exception):
    def __init__(self, message: str, line: int, column: int) -> None:
        super().__init__(message)
        self.message = message
        self.line = line
        self.column = column


def _json_is_ascii_digit(ch: str | None) -> bool:
    return ch is not None and "0" <= ch <= "9"


class _JsonParser:
    def __init__(self, text: str) -> None:
        self.text = text
        self.pos = 0
        self.line = 1
        self.column = 1

    def peek(self) -> str | None:
        if self.pos >= len(self.text):
            return None
        return self.text[self.pos]

    def bump(self) -> str | None:
        ch = self.peek()
        if ch is None:
            return None
        self.pos += 1
        if ch == "\n":
            self.line += 1
            self.column = 1
        else:
            self.column += 1
        return ch

    def error(self, message: str) -> _JsonParseError:
        return _JsonParseError(message, self.line, self.column)

    def skip_ws(self) -> None:
        while self.peek() in (" ", "\t", "\n", "\r"):
            self.bump()

    def parse(self) -> TpzJson:
        self.skip_ws()
        value = self.parse_value(0)
        self.skip_ws()
        if self.peek() is not None:
            raise self.error("unexpected trailing characters after JSON value")
        return value

    def parse_value(self, depth: int) -> TpzJson:
        if depth > 128:
            raise self.error("JSON nesting exceeds the depth limit")
        ch = self.peek()
        if ch is None:
            raise self.error("unexpected end of input")
        if ch == "n":
            self.parse_lit("null")
            return TpzJson("null")
        if ch == "t":
            self.parse_lit("true")
            return TpzJson("bool", True)
        if ch == "f":
            self.parse_lit("false")
            return TpzJson("bool", False)
        if ch == '"':
            return TpzJson("string", self.parse_string())
        if ch == "[":
            return self.parse_array(depth)
        if ch == "{":
            return self.parse_object(depth)
        if ch == "-" or _json_is_ascii_digit(ch):
            return self.parse_number()
        raise self.error("unexpected character `" + ch + "`")

    def parse_lit(self, word: str) -> None:
        for expected in word:
            if self.bump() != expected:
                raise self.error("invalid literal, expected `" + word + "`")

    def parse_string(self) -> str:
        self.bump()
        out = []
        while True:
            ch = self.bump()
            if ch is None:
                raise self.error("unterminated string")
            if ch == '"':
                return "".join(out)
            if ch == "\\":
                esc = self.bump()
                if esc == '"':
                    out.append('"')
                elif esc == "\\":
                    out.append("\\")
                elif esc == "/":
                    out.append("/")
                elif esc == "b":
                    out.append("\b")
                elif esc == "f":
                    out.append("\f")
                elif esc == "n":
                    out.append("\n")
                elif esc == "r":
                    out.append("\r")
                elif esc == "t":
                    out.append("\t")
                elif esc == "u":
                    cp = self.parse_hex4()
                    if 0xD800 <= cp <= 0xDBFF:
                        if self.bump() != "\\" or self.bump() != "u":
                            raise self.error("expected a low surrogate")
                        lo = self.parse_hex4()
                        if not (0xDC00 <= lo <= 0xDFFF):
                            raise self.error("invalid low surrogate")
                        out.append(chr(0x10000 + ((cp - 0xD800) << 10) + (lo - 0xDC00)))
                    elif 0xDC00 <= cp <= 0xDFFF:
                        raise self.error("unexpected low surrogate")
                    else:
                        out.append(chr(cp))
                else:
                    raise self.error("invalid string escape")
            elif ord(ch) < 0x20:
                raise self.error("unescaped control character in string")
            else:
                out.append(ch)

    def parse_hex4(self) -> int:
        value = 0
        for _ in range(4):
            ch = self.bump()
            if ch is None or ch.lower() not in "0123456789abcdef":
                raise self.error("invalid \\u escape")
            value = value * 16 + int(ch, 16)
        return value

    def parse_number(self) -> TpzJson:
        start = self.pos
        if self.peek() == "-":
            self.bump()
        if self.peek() == "0":
            self.bump()
            if _json_is_ascii_digit(self.peek()):
                raise self.error("leading zeros are not allowed")
        elif _json_is_ascii_digit(self.peek()):
            while _json_is_ascii_digit(self.peek()):
                self.bump()
        else:
            raise self.error("invalid number")
        if self.peek() == ".":
            self.bump()
            if not _json_is_ascii_digit(self.peek()):
                raise self.error("expected digits after the decimal point")
            while _json_is_ascii_digit(self.peek()):
                self.bump()
        if self.peek() in ("e", "E"):
            self.bump()
            if self.peek() in ("+", "-"):
                self.bump()
            if not _json_is_ascii_digit(self.peek()):
                raise self.error("expected digits in the exponent")
            while _json_is_ascii_digit(self.peek()):
                self.bump()
        lexeme = self.text[start:self.pos]
        return TpzJson("number", TpzJsonNumber(lexeme, _json_exact_int(lexeme)))

    def parse_array(self, depth: int) -> TpzJson:
        self.bump()
        items = []
        self.skip_ws()
        if self.peek() == "]":
            self.bump()
            return TpzJson("array", items)
        while True:
            self.skip_ws()
            items.append(self.parse_value(depth + 1))
            self.skip_ws()
            ch = self.bump()
            if ch == "]":
                return TpzJson("array", items)
            if ch != ",":
                raise self.error("expected `,` or `]` in array")

    def parse_object(self, depth: int) -> TpzJson:
        self.bump()
        entries = {}
        self.skip_ws()
        if self.peek() == "}":
            self.bump()
            return TpzJson("object", entries)
        while True:
            self.skip_ws()
            if self.peek() != '"':
                raise self.error("expected a string key in object")
            key = self.parse_string()
            self.skip_ws()
            if self.bump() != ":":
                raise self.error("expected `:` after the object key")
            self.skip_ws()
            value = self.parse_value(depth + 1)
            if key in entries:
                raise self.error("duplicate object key")
            entries[key] = value
            self.skip_ws()
            ch = self.bump()
            if ch == "}":
                return TpzJson("object", dict(sorted(entries.items())))
            if ch != ",":
                raise self.error("expected `,` or `}` in object")


class _JsonStringifyError(Exception):
    pass


def _json_exact_int(lexeme: str) -> int | None:
    neg = lexeme.startswith("-")
    body = lexeme[1:] if neg else lexeme
    if "e" in body or "E" in body:
        body, exp_text = body.replace("E", "e").split("e", 1)
        try:
            exp = int(exp_text)
        except ValueError:
            return None
        if exp < I128_MIN or exp > I128_MAX:
            return None
    else:
        exp = 0
    if "." in body:
        whole, frac = body.split(".", 1)
    else:
        whole, frac = body, ""

    digits = (whole + frac).lstrip("0")
    if digits == "":
        return 0

    shift = exp - len(frac)
    if shift >= 0:
        if len(digits) + shift > 19:
            return None
        digits = digits + ("0" * shift)
    elif shift <= -len(digits):
        return None
    else:
        drop = -shift
        keep, dropped = digits[:-drop], digits[-drop:]
        if any(ch != "0" for ch in dropped):
            return None
        digits = keep

    if len(digits) > 19:
        return None
    try:
        value = int(("-" if neg else "") + digits)
    except ValueError:
        return None
    if value < INT_MIN or value > INT_MAX:
        return None
    return value


def _json_escape(raw: str) -> str:
    out = ['"']
    for ch in raw:
        if ch == '"':
            out.append('\\"')
        elif ch == "\\":
            out.append("\\\\")
        elif ch == "\n":
            out.append("\\n")
        elif ch == "\r":
            out.append("\\r")
        elif ch == "\t":
            out.append("\\t")
        elif ord(ch) < 0x20:
            out.append("\\u%04x" % ord(ch))
        else:
            out.append(ch)
    out.append('"')
    return "".join(out)


def _parse_extern_sandbox_policies(text: str | None) -> dict[str, ExternSandboxPolicy] | None:
    if text is None or text == "":
        return None
    try:
        node = json.loads(text)
    except json.JSONDecodeError as error:
        raise ValueError(
            "extern sandbox policies JSON is invalid at line "
            + str(error.lineno)
            + ", column "
            + str(error.colno)
            + ": "
            + error.msg
        ) from None
    if not isinstance(node, list):
        raise ValueError("extern sandbox policies must be an array")
    policies: dict[str, ExternSandboxPolicy] = {}
    for index, item in enumerate(node):
        path = "extern sandbox policies[" + str(index) + "]"
        if not isinstance(item, dict):
            raise ValueError(path + " must be an object")
        _abi_exact_fields(item, {"module", "kind", "artifact_path", "fuel", "memory_bytes"}, path)
        module = item["module"]
        kind = item["kind"]
        artifact_path = item["artifact_path"]
        fuel = item["fuel"]
        memory_bytes = item["memory_bytes"]
        if not isinstance(module, str) or module == "":
            raise ValueError(path + ".module must be a non-empty string")
        if kind not in ("replay", "wasm"):
            raise ValueError(path + ".kind must be `replay` or `wasm`")
        if artifact_path is not None and not isinstance(artifact_path, str):
            raise ValueError(path + ".artifact_path must be a string or null")
        fuel = _parse_optional_u64(fuel, path + ".fuel")
        memory_bytes = _parse_optional_u64(memory_bytes, path + ".memory_bytes")
        policy = ExternSandboxPolicy(module, kind, artifact_path, fuel, memory_bytes)
        _validate_extern_sandbox_policy(policy)
        if module in policies:
            raise ValueError("extern sandbox policy declares a duplicate module")
        policies[module] = policy
    return policies


def _parse_optional_u64(value: object, path: str) -> int | None:
    if value is None:
        return None
    if type(value) is not int or value < 0 or value > U64_MAX:
        raise ValueError(path + " must be a u64 integer or null")
    return value


def _validate_extern_sandbox_policy(policy: ExternSandboxPolicy) -> None:
    if policy.module == "":
        raise ValueError("extern sandbox policy module must not be empty")
    if policy.kind == "wasm" and policy.artifact_path is None:
        raise ValueError(
            "extern sandbox policy for `" + policy.module + "` kind `wasm` requires an artifact"
        )


def _enforce_extern_replay_budget(
    policy: ExternSandboxPolicy,
    module: str,
    function: str,
    args: tuple[object, ...],
    args_json: str,
    result: object,
) -> None:
    if policy.fuel is not None:
        used = _extern_replay_fuel_used(args, result)
        if used > policy.fuel:
            raise ValueError(
                "extern replay fuel limit exceeded for `"
                + module
                + "."
                + function
                + "`: used "
                + str(used)
                + ", budget "
                + str(policy.fuel)
            )
    if policy.memory_bytes is not None:
        result_json = _abi_encode_value(result, "$", 0)
        used = _extern_replay_memory_bytes_used(args_json, result_json)
        if used > policy.memory_bytes:
            raise ValueError(
                "extern replay memory_bytes limit exceeded for `"
                + module
                + "."
                + function
                + "`: used "
                + str(used)
                + ", budget "
                + str(policy.memory_bytes)
            )


def _extern_replay_fuel_used(args: tuple[object, ...], result: object) -> int:
    used = 1
    for arg in args:
        used = _abi_charge_add(used, _abi_value_nodes(arg, 0))
    return _abi_charge_add(used, _abi_value_nodes(result, 0))


def _extern_replay_memory_bytes_used(args_json: str, result_json: str) -> int:
    return _abi_charge_add(len(args_json.encode("utf-8")), len(result_json.encode("utf-8")))


def _abi_value_nodes(value: object, depth: int) -> int:
    if depth > 128:
        raise ValueError("ABI_LIMIT: extern replay resource envelope exceeds the ABI value depth limit")
    child_depth = depth + 1
    total = 1
    if isinstance(value, (Some, Ok, Err)):
        return _abi_charge_add(total, _abi_value_nodes(value.value, child_depth))
    if isinstance(value, list):
        for item in value:
            total = _abi_charge_add(total, _abi_value_nodes(item, child_depth))
        return total
    if _is_topaz_record(value):
        for py_field, _source_field in getattr(value, "__topaz_record_fields__"):
            total = _abi_charge_add(total, _abi_value_nodes(getattr(value, py_field), child_depth))
        return total
    if (
        type(value) is int
        or type(value) is bool
        or isinstance(value, str)
        or value is TPZ_UNIT
        or value is TPZ_NULL
        or value is None
        or isinstance(value, TpzJson)
        or isinstance(value, TpzBytes)
    ):
        return total
    raise ValueError("ABI_UNSUPPORTED: extern replay resource envelope contains `" + tpz_kind(value) + "`")


def _abi_charge_add(lhs: int, rhs: int) -> int:
    total = lhs + rhs
    if total > U64_MAX:
        raise ValueError("extern replay resource envelope exceeds u64")
    return total


def _mangle(name: str) -> str:
    return "_t_" + "".join(format(byte, "02x") for byte in name.encode("utf-8"))


def _abi_encode_value(value: object, path: str, depth: int) -> str:
    if depth > 128:
        raise ValueError("ABI_LIMIT: " + path + ": structure exceeds the ABI value depth limit")
    if type(value) is int:
        return '{"$":"int","value":' + _json_escape(str(value)) + "}"
    if type(value) is bool:
        return '{"$":"bool","value":' + ("true" if value else "false") + "}"
    if isinstance(value, str):
        return '{"$":"string","value":' + _json_escape(value) + "}"
    if value is TPZ_UNIT:
        return '{"$":"unit"}'
    if value is TPZ_NULL:
        return '{"$":"null"}'
    if value is None:
        return '{"$":"none"}'
    if isinstance(value, Some):
        return '{"$":"some","value":' + _abi_encode_value(value.value, path + ".value", depth + 1) + "}"
    if isinstance(value, Ok):
        return '{"$":"ok","value":' + _abi_encode_value(value.value, path + ".value", depth + 1) + "}"
    if isinstance(value, Err):
        return '{"$":"err","value":' + _abi_encode_value(value.value, path + ".value", depth + 1) + "}"
    if isinstance(value, list):
        return (
            '{"$":"array","items":['
            + ",".join(_abi_encode_value(item, path + ".items[" + str(i) + "]", depth + 1) for i, item in enumerate(value))
            + "]}"
        )
    if isinstance(value, TpzBytes):
        return '{"$":"bytes","hex":' + _json_escape(tpz_bytes_to_hex(value)) + "}"
    if isinstance(value, TpzJson):
        return '{"$":"json","value":' + _json_write_node(value) + "}"
    if _is_topaz_newtype(value):
        return (
            '{"$":"newtype","id":'
            + _json_escape(value.newtype_id)
            + ',"value":'
            + _abi_encode_value(value.value, path + ".value", depth + 1)
            + "}"
        )
    if _is_topaz_record(value):
        fields = []
        for py_name, source_name in sorted(getattr(value, "__topaz_record_fields__"), key=lambda item: item[1]):
            fields.append(_json_escape(source_name) + ":" + _abi_encode_value(getattr(value, py_name), path + ".fields." + source_name, depth + 1))
        if getattr(value, "__topaz_record_id__", None) is not None:
            ordered = []
            for py_name, source_name in getattr(value, "__topaz_record_fields__"):
                ordered.append(
                    '{"name":'
                    + _json_escape(source_name)
                    + ',"value":'
                    + _abi_encode_value(getattr(value, py_name), path + ".fields." + source_name, depth + 1)
                    + "}"
                )
            return (
                '{"$":"nominal-record","id":'
                + _json_escape(value.__topaz_record_id__)
                + ',"fields":['
                + ",".join(ordered)
                + "]}"
            )
        return '{"$":"record","fields":{' + ",".join(fields) + "}}"
    raise ValueError("ABI_UNSUPPORTED: " + path + ": `" + tpz_kind(value) + "` is not encodable by the public ABI")


def _abi_args_encode(args: tuple[object, ...]) -> str:
    return "[" + ",".join(_abi_encode_value(arg, "$[" + str(i) + "]", 0) for i, arg in enumerate(args)) + "]"


def _abi_exact_fields(node: dict[str, object], fields: set[str], path: str) -> None:
    actual = set(node.keys())
    if actual != fields:
        raise ValueError(
            "ABI_FIELDS: "
            + path
            + " expected fields "
            + ", ".join(sorted(fields))
            + ", got "
            + ", ".join(sorted(actual))
        )


def _abi_decode_value(node: object, path: str, depth: int) -> object:
    if depth > 128:
        raise ValueError("ABI_LIMIT: " + path + ": structure exceeds the ABI value depth limit")
    if not isinstance(node, dict):
        raise ValueError("ABI_SHAPE: " + path + " must be an object")
    tag = node.get("$")
    if not isinstance(tag, str):
        raise ValueError("ABI_TAG: " + path + ".$ must be a string")
    if tag == "int":
        _abi_exact_fields(node, {"$", "value"}, path)
        raw = node["value"]
        if not isinstance(raw, str):
            raise ValueError("ABI_INT: " + path + ".value must be a string")
        try:
            value = int(raw, 10)
        except ValueError:
            raise ValueError("ABI_INT: " + path + ".value is not an i64 decimal string") from None
        if str(value) != raw or value < INT_MIN or value > INT_MAX:
            raise ValueError("ABI_INT: " + path + ".value is not canonical decimal")
        return value
    if tag == "bool":
        _abi_exact_fields(node, {"$", "value"}, path)
        value = node["value"]
        if type(value) is not bool:
            raise ValueError("ABI_BOOL: " + path + ".value must be boolean")
        return value
    if tag == "string":
        _abi_exact_fields(node, {"$", "value"}, path)
        value = node["value"]
        if not isinstance(value, str):
            raise ValueError("ABI_STRING: " + path + ".value must be a string")
        return value
    if tag == "unit":
        _abi_exact_fields(node, {"$"}, path)
        return TPZ_UNIT
    if tag == "null":
        _abi_exact_fields(node, {"$"}, path)
        return TPZ_NULL
    if tag == "none":
        _abi_exact_fields(node, {"$"}, path)
        return None
    if tag == "some":
        _abi_exact_fields(node, {"$", "value"}, path)
        return Some(_abi_decode_value(node["value"], path + ".value", depth + 1))
    if tag == "ok":
        _abi_exact_fields(node, {"$", "value"}, path)
        return Ok(_abi_decode_value(node["value"], path + ".value", depth + 1))
    if tag == "err":
        _abi_exact_fields(node, {"$", "value"}, path)
        return Err(_abi_decode_value(node["value"], path + ".value", depth + 1))
    if tag == "array":
        _abi_exact_fields(node, {"$", "items"}, path)
        items = node["items"]
        if not isinstance(items, list):
            raise ValueError("ABI_ARRAY: " + path + ".items must be an array")
        return [_abi_decode_value(item, path + ".items[" + str(i) + "]", depth + 1) for i, item in enumerate(items)]
    if tag == "record":
        _abi_exact_fields(node, {"$", "fields"}, path)
        fields = node["fields"]
        if not isinstance(fields, dict):
            raise ValueError("ABI_RECORD: " + path + ".fields must be an object")
        metadata = tuple((_mangle(source_name), source_name) for source_name in sorted(fields.keys()))
        values = {
            py_field: _abi_decode_value(fields[source_name], path + ".fields." + source_name, depth + 1)
            for py_field, source_name in metadata
        }
        return _record_class_for_fields(metadata)(**values)
    if tag == "nominal-record":
        _abi_exact_fields(node, {"$", "id", "fields"}, path)
        record_id = node["id"]
        fields = node["fields"]
        if not isinstance(record_id, str):
            raise ValueError("ABI_NOMINAL_RECORD: " + path + ".id must be a string")
        if not isinstance(fields, list):
            raise ValueError("ABI_NOMINAL_RECORD: " + path + ".fields must be an array")
        seen: set[str] = set()
        metadata = []
        values = {}
        for i, field in enumerate(fields):
            field_path = path + ".fields[" + str(i) + "]"
            if not isinstance(field, dict):
                raise ValueError("ABI_NOMINAL_RECORD: " + field_path + " must be an object")
            _abi_exact_fields(field, {"name", "value"}, field_path)
            source_name = field["name"]
            if not isinstance(source_name, str):
                raise ValueError("ABI_NOMINAL_RECORD: " + field_path + ".name must be a string")
            if source_name in seen:
                raise ValueError("ABI_NOMINAL_RECORD: " + field_path + ".name duplicates `" + source_name + "`")
            seen.add(source_name)
            py_field = _mangle(source_name)
            metadata.append((py_field, source_name))
            values[py_field] = _abi_decode_value(field["value"], field_path + ".value", depth + 1)
        return _record_class_for_fields(tuple(metadata), record_id)(**values)
    if tag == "newtype":
        _abi_exact_fields(node, {"$", "id", "value"}, path)
        newtype_id = node["id"]
        if not isinstance(newtype_id, str):
            raise ValueError("ABI_NEWTYPE: " + path + ".id must be a string")
        return TpzNewtype(newtype_id, _abi_decode_value(node["value"], path + ".value", depth + 1))
    if tag == "bytes":
        _abi_exact_fields(node, {"$", "hex"}, path)
        raw = node["hex"]
        if not isinstance(raw, str):
            raise ValueError("ABI_BYTES: " + path + ".hex must be a string")
        decoded = tpz_bytes_from_hex(raw, (0, 0, 0))
        if isinstance(decoded, Err):
            raise ValueError("ABI_BYTES: " + path + ".hex is not lowercase hex")
        if tpz_bytes_to_hex(decoded.value) != raw:
            raise ValueError("ABI_BYTES: " + path + ".hex is non-lowercase-hex")
        return decoded.value
    if tag == "json":
        _abi_exact_fields(node, {"$", "value"}, path)
        return _json_to_tpz_json(node["value"], path + ".value", depth + 1)
    raise ValueError("ABI_TAG: " + path + " has unsupported tag `" + tag + "`")


def _json_to_tpz_json(node: object, path: str, depth: int) -> TpzJson:
    if depth > 128:
        raise ValueError("ABI_LIMIT: " + path + ": structure exceeds the ABI value depth limit")
    if node is None:
        return TpzJson("null")
    if type(node) is bool:
        return TpzJson("bool", node)
    if isinstance(node, str):
        return TpzJson("string", node)
    if type(node) is int:
        return TpzJson("number", TpzJsonNumber(str(node), node))
    if isinstance(node, list):
        return TpzJson("array", [_json_to_tpz_json(item, path + "[" + str(i) + "]", depth + 1) for i, item in enumerate(node)])
    if isinstance(node, dict):
        return TpzJson("object", {key: _json_to_tpz_json(value, path + "." + key, depth + 1) for key, value in sorted(node.items())})
    raise ValueError("ABI_JSON: " + path + " has unsupported JSON value")


def _json_write_node(node: TpzJson) -> str:
    if node.kind == "null":
        return "null"
    if node.kind == "bool":
        return "true" if node.value else "false"
    if node.kind == "string":
        return _json_escape(node.value)
    if node.kind == "number":
        return node.value.lexeme
    if node.kind == "array":
        return "[" + ",".join(_json_write_node(item) for item in node.value) + "]"
    if node.kind == "object":
        return "{" + ",".join(_json_escape(k) + ":" + _json_write_node(v) for k, v in sorted(node.value.items())) + "}"
    raise TypeError("unknown JSON node")


def _json_encode_value(value: object, path: str, depth: int) -> str:
    if depth > 128:
        raise _JsonStringifyError(
            "JSON_LIMIT: " + path + ": structure exceeds the JSON.stringify depth limit"
        )
    if isinstance(value, TpzJson):
        return _json_write_node(value)
    if type(value) is bool:
        return "true" if value else "false"
    if type(value) is int:
        return str(value)
    if value is TPZ_UNIT or value is TPZ_NULL or value is None:
        return "null"
    if isinstance(value, str):
        return _json_escape(value)
    if isinstance(value, Some):
        return _json_encode_value(value.value, path, depth)
    if isinstance(value, TpzNewtype):
        return _json_encode_value(value.value, path, depth)
    if isinstance(value, list):
        return "[" + ",".join(_json_encode_value(item, path + "[" + str(i) + "]", depth + 1) for i, item in enumerate(value)) + "]"
    if type(value) is float:
        raise _JsonStringifyError(
            "JSON_UNSUPPORTED: " + path + ": float is not supported by JSON.stringify v1"
        )
    raise _JsonStringifyError(
        "JSON_UNSUPPORTED: " + path + ": `" + tpz_kind(value) + "` is not JSON-encodable"
    )


def tpz_json_parse(text: object, span: tuple[int, int, int]) -> Ok | Err:
    if not isinstance(text, str):
        tpz_fault("TPZ5001", "`JSON.parse` takes a string; got `" + tpz_kind(text) + "` (§22)", span)
    try:
        return Ok(_JsonParser(text).parse())
    except _JsonParseError as error:
        return Err(TpzJsonParseErrorRecord(error.column, error.line, error.message))


def tpz_json_stringify(value: object) -> Ok | Err:
    try:
        return Ok(_json_encode_value(value, "$", 0))
    except _JsonStringifyError as error:
        return Err(str(error))


def _tpz_json_decode_value(node: TpzJson, schema: object, path: str, depth: int) -> object:
    if depth > 128:
        raise ValueError(
            "JSON_LIMIT: " + path + ": structure exceeds the JSON.decode depth limit"
        )
    if schema == "json":
        return node
    if schema == "int":
        if node.kind != "number":
            raise ValueError(path + ": expected int, found " + node.kind)
        if node.value.int_value is None:
            raise ValueError(path + ": expected an integer, found a non-integer number")
        return node.value.int_value
    if schema == "string":
        if node.kind != "string":
            raise ValueError(path + ": expected string, found " + node.kind)
        return node.value
    if schema == "bool":
        if node.kind != "bool":
            raise ValueError(path + ": expected bool, found " + node.kind)
        return node.value
    if schema == "unit":
        if node.kind != "null":
            raise ValueError(path + ": expected null, found " + node.kind)
        return TPZ_UNIT
    if schema == "null":
        if node.kind != "null":
            raise ValueError(path + ": expected null, found " + node.kind)
        return TPZ_NULL
    if not isinstance(schema, tuple) or not schema:
        raise ValueError("JSON_SCHEMA: malformed typed JSON schema")
    tag = schema[0]
    if tag == "option":
        if node.kind == "null":
            return None
        return Some(_tpz_json_decode_value(node, schema[1], path, depth))
    if tag == "array":
        if node.kind != "array":
            raise ValueError(path + ": expected array, found " + node.kind)
        return [
            _tpz_json_decode_value(item, schema[1], path + "[" + str(i) + "]", depth + 1)
            for i, item in enumerate(node.value)
        ]
    if tag == "map":
        if node.kind != "object":
            raise ValueError(path + ": expected object, found " + node.kind)
        out = TpzMap([])
        for key, item in sorted(node.value.items()):
            decoded = _tpz_json_decode_value(
                item, schema[1], path + "." + key, depth + 1
            )
            out.entries.append((_key(key, (0, 0, 0)), decoded))
        return out
    if tag in ("struct", "record"):
        if node.kind != "object":
            raise ValueError(path + ": expected object, found " + node.kind)
        record_id = None if tag == "struct" else schema[1]
        declaration_identity = None
        if tag == "struct":
            fields = schema[1]
        elif len(schema) == 4:
            declaration_identity = schema[2]
            fields = schema[3]
        else:
            fields = schema[2]
        metadata = tuple((field[1], field[0]) for field in fields)
        values = {}
        for field in fields:
            source_name, py_name, field_schema = field[:3]
            child = node.value.get(source_name)
            if child is None:
                if len(field) == 4:
                    values[py_name] = field[3]
                    continue
                raise ValueError(path + ": missing required field `" + source_name + "`")
            values[py_name] = _tpz_json_decode_value(
                child, field_schema, path + "." + source_name, depth + 1
            )
        return _record_class_for_fields(
            metadata, record_id, declaration_identity
        )(**values)
    if tag == "newtype":
        declaration_identity = schema[2] if len(schema) == 4 else None
        base_schema = schema[3] if len(schema) == 4 else schema[2]
        return TpzNewtype(
            schema[1],
            _tpz_json_decode_value(node, base_schema, path, depth),
            None,
            declaration_identity,
        )
    if tag == "enum":
        if node.kind != "object":
            raise ValueError(path + ": expected an enum object, found " + node.kind)
        tag_node = node.value.get("tag")
        if tag_node is None:
            raise ValueError(path + ": missing enum `tag` field")
        if tag_node.kind != "string":
            raise ValueError(path + ".tag: expected string, found " + tag_node.kind)
        declaration_identity = schema[2] if len(schema) == 4 else None
        variants = schema[3] if len(schema) == 4 else schema[2]
        variant = next((item for item in variants if item[0] == tag_node.value), None)
        if variant is None:
            raise ValueError(path + ": unknown variant tag `" + tag_node.value + "`")
        values_node = node.value.get("values")
        if values_node is None:
            values = []
        elif values_node.kind == "array":
            values = values_node.value
        else:
            raise ValueError(path + ".values: expected array, found " + values_node.kind)
        payload_schemas = variant[2]
        if len(values) != len(payload_schemas):
            raise ValueError(
                path
                + ": variant `"
                + tag_node.value
                + "` expects "
                + str(len(payload_schemas))
                + " value(s), found "
                + str(len(values))
            )
        payloads = tuple(
            _tpz_json_decode_value(
                item,
                payload_schema,
                path + ".values[" + str(i) + "]",
                depth + 1,
            )
            for i, (item, payload_schema) in enumerate(zip(values, payload_schemas))
        )
        return TpzEnum(
            schema[1], variant[0], variant[1], payloads, None, declaration_identity
        )
    raise ValueError("JSON_SCHEMA: malformed typed JSON schema")


def _tpz_json_decode_result(node: TpzJson, schema: object) -> Ok | Err:
    try:
        return Ok(_tpz_json_decode_value(node, schema, "$", 0))
    except ValueError as error:
        return Err(str(error))


def tpz_json_parse_as(value: object, schema: object, span: tuple[int, int, int]) -> Ok | Err:
    if not isinstance(value, str):
        tpz_fault(
            "TPZ5001",
            "`JSON.parseAs` takes a string; got `" + tpz_kind(value) + "` (§22)",
            span,
        )
    try:
        node = _JsonParser(value).parse()
    except _JsonParseError as error:
        return Err(
            "$: invalid JSON at line "
            + str(error.line)
            + ", column "
            + str(error.column)
            + ": "
            + error.message
        )
    return _tpz_json_decode_result(node, schema)


def tpz_json_decode(value: object, schema: object, span: tuple[int, int, int]) -> Ok | Err:
    if not isinstance(value, TpzJson):
        tpz_fault(
            "TPZ5001",
            "`JSON.decode` takes a JSONValue; got `" + tpz_kind(value) + "` (§22)",
            span,
        )
    return _tpz_json_decode_result(value, schema)


def _json_receiver(value: object, member: str, span: tuple[int, int, int]) -> TpzJson:
    if not isinstance(value, TpzJson):
        tpz_no_member(value, member, span)
    return value


def tpz_json_kind(value: object, span: tuple[int, int, int]) -> str:
    return _json_receiver(value, "kind", span).kind


def tpz_json_is_null(value: object, span: tuple[int, int, int]) -> bool:
    return _json_receiver(value, "isNull", span).kind == "null"


def tpz_json_as_string(value: object, span: tuple[int, int, int]) -> Some | None:
    node = _json_receiver(value, "asString", span)
    return Some(node.value) if node.kind == "string" else None


def tpz_json_as_bool(value: object, span: tuple[int, int, int]) -> Some | None:
    node = _json_receiver(value, "asBool", span)
    return Some(node.value) if node.kind == "bool" else None


def tpz_json_as_int(value: object, span: tuple[int, int, int]) -> Some | None:
    node = _json_receiver(value, "asInt", span)
    if node.kind == "number" and node.value.int_value is not None:
        return Some(node.value.int_value)
    return None


def tpz_json_number_text(value: object, span: tuple[int, int, int]) -> Some | None:
    node = _json_receiver(value, "numberText", span)
    return Some(node.value.lexeme) if node.kind == "number" else None


def tpz_json_get(value: object, key: object, member_span: tuple[int, int, int], call_span: tuple[int, int, int]) -> Some | None:
    node = _json_receiver(value, "get", member_span)
    if node.kind != "object":
        return None
    if not isinstance(key, str):
        tpz_fault("TPZ5001", "`JSONValue.get` takes a string key; got `" + tpz_kind(key) + "`", call_span)
    return Some(node.value[key]) if key in node.value else None


def tpz_get(value: object, key: object, span: tuple[int, int, int]) -> Some | None:
    if isinstance(value, TpzJson):
        return tpz_json_get(value, key, span, span)
    if isinstance(value, TpzBytes):
        return tpz_bytes_get(value, key, span)
    if isinstance(value, TpzMap):
        return tpz_map_get(value, key, span)
    return tpz_array_get(value, key, span)


def tpz_json_at(value: object, index: object, member_span: tuple[int, int, int], call_span: tuple[int, int, int]) -> Some | None:
    node = _json_receiver(value, "at", member_span)
    if node.kind != "array":
        return None
    idx = _int(index, call_span)
    return Some(node.value[idx]) if 0 <= idx < len(node.value) else None


def tpz_json_length(value: object, span: tuple[int, int, int]) -> Some | None:
    node = _json_receiver(value, "length", span)
    if node.kind in ("array", "object"):
        return Some(len(node.value))
    return None


def tpz_json_as_array(value: object, span: tuple[int, int, int]) -> Some | None:
    node = _json_receiver(value, "asArray", span)
    return Some(list(node.value)) if node.kind == "array" else None


def tpz_json_keys(value: object, span: tuple[int, int, int]) -> Some | None:
    node = _json_receiver(value, "keys", span)
    return Some(sorted(node.value)) if node.kind == "object" else None


def tpz_json_values(value: object, span: tuple[int, int, int]) -> Some | None:
    node = _json_receiver(value, "values", span)
    if node.kind != "object":
        return None
    return Some([node.value[key] for key in sorted(node.value)])


def tpz_length(value: object, span: tuple[int, int, int]) -> object:
    if isinstance(value, TpzJson):
        return tpz_json_length(value, span)
    if isinstance(value, TpzBytes):
        return tpz_bytes_length(value, span)
    tpz_no_member(value, "length", span)


def _stdlib_string(
    value: object,
    owner: str,
    method: str,
    param: str,
    span: tuple[int, int, int],
) -> str:
    if not isinstance(value, str):
        tpz_fault(
            "TPZ5001",
            "`"
            + owner
            + "."
            + method
            + "` parameter `"
            + param
            + "` expects string; found `"
            + tpz_kind(value)
            + "`",
            span,
        )
    return value


def _stdlib_string_array(
    value: object, owner: str, method: str, span: tuple[int, int, int]
) -> list[str]:
    if not isinstance(value, list) or any(not isinstance(item, str) for item in value):
        tpz_fault(
            "TPZ5001",
            "`"
            + owner
            + "."
            + method
            + "` takes `Array<string>`; found `"
            + tpz_kind(value)
            + "`",
            span,
        )
    return value


def tpz_cli_has_flag(
    args: object, name: object, span: tuple[int, int, int]
) -> bool:
    argv = _stdlib_string_array(args, "Cli", "hasFlag", span)
    flag = _stdlib_string(name, "Cli", "hasFlag", "name", span)
    before_dashdash = argv[: argv.index("--") if "--" in argv else len(argv)]
    return any(arg == flag for arg in before_dashdash)


def _cli_option_values(argv: list[str], name: str) -> list[str]:
    out: list[str] = []
    eq_prefix = name + "="
    i = 0
    while i < len(argv):
        arg = argv[i]
        if arg == "--":
            break
        if arg.startswith(eq_prefix):
            out.append(arg[len(eq_prefix) :])
            i += 1
            continue
        if arg == name:
            if i + 1 < len(argv) and (
                not argv[i + 1].startswith("-") or argv[i + 1] == "-"
            ):
                out.append(argv[i + 1])
                i += 2
                continue
        i += 1
    return out


def tpz_cli_option(
    args: object, name: object, span: tuple[int, int, int]
) -> Some | None:
    argv = _stdlib_string_array(args, "Cli", "option", span)
    option = _stdlib_string(name, "Cli", "option", "name", span)
    values = _cli_option_values(argv, option)
    return Some(values[0]) if values else None


def tpz_cli_options(
    args: object, name: object, span: tuple[int, int, int]
) -> list[str]:
    argv = _stdlib_string_array(args, "Cli", "options", span)
    option = _stdlib_string(name, "Cli", "options", "name", span)
    return _cli_option_values(argv, option)


def tpz_cli_positionals(args: object, span: tuple[int, int, int]) -> list[str]:
    argv = _stdlib_string_array(args, "Cli", "positionals", span)
    out: list[str] = []
    i = 0
    after_dashdash = False
    while i < len(argv):
        arg = argv[i]
        if after_dashdash:
            out.append(arg)
            i += 1
            continue
        if arg == "--":
            after_dashdash = True
            i += 1
            continue
        if arg.startswith("-") and arg != "-":
            if (
                "=" not in arg
                and i + 1 < len(argv)
                and (not argv[i + 1].startswith("-") or argv[i + 1] == "-")
            ):
                i += 2
            else:
                i += 1
            continue
        out.append(arg)
        i += 1
    return out


def tpz_hash_sha256(value: object, span: tuple[int, int, int]) -> TpzBytes:
    raw = _bytes_receiver(value, "sha256", span).data
    return TpzBytes(hashlib.sha256(raw).digest())


def tpz_hash_sha512(value: object, span: tuple[int, int, int]) -> TpzBytes:
    raw = _bytes_receiver(value, "sha512", span).data
    return TpzBytes(hashlib.sha512(raw).digest())


def tpz_hash_hmac_sha256(
    key: object, message: object, span: tuple[int, int, int]
) -> TpzBytes:
    key_raw = _bytes_receiver(key, "hmacSha256", span).data
    message_raw = _bytes_receiver(message, "hmacSha256", span).data
    return TpzBytes(hmac.new(key_raw, message_raw, hashlib.sha256).digest())


def tpz_hash_crc32(value: object, span: tuple[int, int, int]) -> int:
    raw = _bytes_receiver(value, "crc32", span).data
    crc = 0xFFFFFFFF
    for byte in raw:
        crc ^= byte
        for _ in range(8):
            mask = -(crc & 1) & 0xFFFFFFFF
            crc = ((crc >> 1) ^ (0xEDB88320 & mask)) & 0xFFFFFFFF
    return (~crc) & 0xFFFFFFFF


def _math_float(value: object, method: str, span: tuple[int, int, int]) -> float:
    if type(value) is not float:
        tpz_fault(
            "TPZ5001",
            "`Math."
            + method
            + "` takes a `float`, found `"
            + tpz_kind(value)
            + "`",
            span,
        )
    return value


def tpz_math_sqrt(value: object, span: tuple[int, int, int]) -> Ok | Err:
    number = _math_float(value, "sqrt", span)
    if math.isnan(number) or number < 0.0:
        if math.isnan(number):
            display = "NaN"
        elif number.is_integer():
            display = str(int(number))
        else:
            display = tpz_format_f64(number)
        return Err(
            "Math.sqrt: domain error (argument " + display + " is negative)"
        )
    return Ok(math.sqrt(number))


def tpz_math_abs(value: object, span: tuple[int, int, int]) -> float:
    return abs(_math_float(value, "abs", span))


def tpz_math_floor(value: object, span: tuple[int, int, int]) -> float:
    number = _math_float(value, "floor", span)
    if number == 0.0 or not math.isfinite(number):
        return number
    return float(math.floor(number))


def tpz_math_ceil(value: object, span: tuple[int, int, int]) -> float:
    number = _math_float(value, "ceil", span)
    if number == 0.0 or not math.isfinite(number):
        return number
    return float(math.ceil(number))


def tpz_math_round(value: object, span: tuple[int, int, int]) -> float:
    number = _math_float(value, "round", span)
    if number == 0.0 or not math.isfinite(number):
        return number
    return float(math.floor(number + 0.5) if number >= 0.0 else math.ceil(number - 0.5))


def tpz_math_sin(value: object, span: tuple[int, int, int]) -> float:
    number = _math_float(value, "sin", span)
    try:
        return math.sin(number)
    except ValueError:
        return float("nan")


def tpz_math_cos(value: object, span: tuple[int, int, int]) -> float:
    number = _math_float(value, "cos", span)
    try:
        return math.cos(number)
    except ValueError:
        return float("nan")


def tpz_math_tan(value: object, span: tuple[int, int, int]) -> float:
    number = _math_float(value, "tan", span)
    try:
        return math.tan(number)
    except ValueError:
        return float("nan")


def tpz_math_is_nan(value: object, span: tuple[int, int, int]) -> bool:
    return math.isnan(_math_float(value, "isNaN", span))


def tpz_math_is_finite(value: object, span: tuple[int, int, int]) -> bool:
    return math.isfinite(_math_float(value, "isFinite", span))


def tpz_math_parse_float(value: object, span: tuple[int, int, int]) -> Ok | Err:
    source = _stdlib_string(value, "Math", "parseFloat", "s", span)
    try:
        parsed = float(source.strip())
    except ValueError:
        return Err("Math.parseFloat: could not parse `" + source + "` as a float")
    if not math.isfinite(parsed):
        return Err("Math.parseFloat: could not parse `" + source + "` as a float")
    return Ok(parsed)


def tpz_math_min(
    left: object, right: object, span: tuple[int, int, int]
) -> float:
    a = _math_float(left, "min", span)
    b = _math_float(right, "min", span)
    return a if a < b else b


def tpz_math_max(
    left: object, right: object, span: tuple[int, int, int]
) -> float:
    a = _math_float(left, "max", span)
    b = _math_float(right, "max", span)
    return a if a > b else b


def tpz_regex_compile(pattern: object, span: tuple[int, int, int]) -> Ok | Err:
    source = _stdlib_string(pattern, "Regex", "compile", "pattern", span)
    try:
        return Ok(TpzRegex(source, re.compile(source, re.ASCII)))
    except re.error as error:
        return Err("Regex.compile: invalid pattern: " + str(error))


def _regex_receiver(
    value: object, method: str, span: tuple[int, int, int]
) -> TpzRegex:
    if not isinstance(value, TpzRegex):
        tpz_no_member(value, method, span)
    return value


def tpz_regex_is_match(
    value: object, text: object, span: tuple[int, int, int]
) -> bool:
    regex = _regex_receiver(value, "isMatch", span)
    source = _stdlib_string(text, "Regex", "isMatch", "text", span)
    return regex.compiled.search(source) is not None


def tpz_regex_split(
    value: object, text: object, span: tuple[int, int, int]
) -> list[str]:
    regex = _regex_receiver(value, "split", span)
    source = _stdlib_string(text, "Regex", "split", "text", span)
    return regex.compiled.split(source)


def tpz_split(value: object, separator: object, span: tuple[int, int, int]) -> list[str]:
    if isinstance(value, TpzRegex):
        return tpz_regex_split(value, separator, span)
    return tpz_string_split(value, separator, span)


def tpz_regex_replace_all(
    value: object,
    text: object,
    replacement: object,
    span: tuple[int, int, int],
) -> str:
    regex = _regex_receiver(value, "replaceAll", span)
    source = _stdlib_string(text, "Regex", "replaceAll", "text", span)
    replace = _stdlib_string(replacement, "Regex", "replaceAll", "replacement", span)
    return regex.compiled.sub(lambda _match: replace, source)


def tpz_csv_parse_with_header(text: object, span: tuple[int, int, int]) -> Ok | Err:
    source = _stdlib_string(text, "CSV", "parseWithHeader", "text", span)
    try:
        rows = list(csv.reader(io.StringIO(source), strict=True))
    except csv.Error as error:
        return Err("CSV.parse: " + str(error))
    if not rows:
        return Ok([])
    header = rows[0]
    out: list[TpzMap] = []
    for row in rows[1:]:
        if len(row) > len(header):
            return Err("CSV.parseWithHeader: row has more fields than the header")
        entries = [
            (_key(name, span), row[index] if index < len(row) else "")
            for index, name in enumerate(header)
        ]
        out.append(TpzMap(entries))
    return Ok(out)


def _toml_json_value(value: object) -> TpzJson:
    if value is None:
        return TpzJson("null")
    if type(value) is bool:
        return TpzJson("bool", value)
    if type(value) is int:
        return TpzJson("number", TpzJsonNumber(str(value), value))
    if type(value) is float:
        return TpzJson("number", TpzJsonNumber(repr(value), None))
    if isinstance(value, str):
        return TpzJson("string", value)
    if isinstance(value, list):
        return TpzJson("array", [_toml_json_value(item) for item in value])
    if isinstance(value, dict):
        return TpzJson(
            "object",
            {
                str(key): _toml_json_value(item)
                for key, item in sorted(value.items())
            },
        )
    return TpzJson("string", str(value))


def tpz_toml_parse(text: object, span: tuple[int, int, int]) -> Ok | Err:
    source = _stdlib_string(text, "TOML", "parse", "text", span)
    try:
        return Ok(TpzToml(tomllib.loads(source)))
    except tomllib.TOMLDecodeError as error:
        return Err("TOML.parse: " + str(error))


def tpz_toml_to_json(value: object, span: tuple[int, int, int]) -> TpzJson:
    if not isinstance(value, TpzToml):
        tpz_fault(
            "TPZ5001",
            "`TOML.toJson` takes `TOMLValue`, found `" + tpz_kind(value) + "`",
            span,
        )
    return _toml_json_value(value.value)


def tpz_url_parse(text: object, span: tuple[int, int, int]) -> Ok | Err:
    source = _stdlib_string(text, "URL", "parse", "text", span)
    if not source or any(char.isspace() or ord(char) < 32 for char in source):
        return Err("URL.parse: URL is empty or contains whitespace/control characters")
    try:
        parsed = urlsplit(source)
        if not parsed.scheme:
            return Err("URL.parse: missing scheme")
        if parsed.netloc and (not parsed.hostname or "@" in parsed.netloc):
            return Err("URL.parse: authority must contain a host and no userinfo")
        scheme = parsed.scheme.lower()
        host = parsed.hostname.lower() if parsed.hostname else None
        authority = parsed.netloc.lower()
        path = parsed.path or ("/" if parsed.netloc else "")
        canonical = (
            scheme + ":" + ("//" + authority if parsed.netloc else "") + path
        )
        if parsed.query:
            canonical += "?" + parsed.query
        if parsed.fragment:
            canonical += "#" + parsed.fragment
        return Ok(
            TpzUrl(
                canonical,
                scheme,
                host,
                path,
                tuple(parse_qsl(parsed.query, keep_blank_values=True)),
                parsed.fragment or None,
            )
        )
    except ValueError as error:
        return Err("URL.parse: " + str(error))


def _url_receiver(
    value: object, method: str, span: tuple[int, int, int]
) -> TpzUrl:
    if not isinstance(value, TpzUrl):
        tpz_no_member(value, method, span)
    return value


def tpz_url_path(value: object, span: tuple[int, int, int]) -> str:
    return _url_receiver(value, "path", span).path_value


def tpz_url_to_string(value: object, span: tuple[int, int, int]) -> str:
    return _url_receiver(value, "toString", span).canonical


def tpz_kind(value: object) -> str:
    if type(value) is bool:
        return "bool"
    if type(value) is int:
        return "int"
    if type(value) is float:
        return "float"
    if isinstance(value, str):
        return "string"
    if value is TPZ_UNIT:
        return "()"
    if value is TPZ_NULL:
        return "null"
    if value is None:
        return "Option"
    if isinstance(value, Some):
        return "Option"
    if isinstance(value, (Ok, Err)):
        return "Result"
    if isinstance(value, list):
        return "Array"
    if isinstance(value, TpzFile):
        return "File"
    if isinstance(value, TpzBytes):
        return "Bytes"
    if isinstance(value, TpzByteBuffer):
        return "ByteBuffer"
    if isinstance(value, TpzMap):
        return "Map"
    if isinstance(value, TpzSet):
        return "Set"
    if isinstance(value, TpzRange):
        return "range"
    if isinstance(value, TpzComposed):
        return "function"
    if isinstance(value, TpzHostCallable):
        return "function"
    if isinstance(value, TpzExternFunction):
        return "function"
    if isinstance(value, TpzJson):
        return "JSONValue"
    if isinstance(value, TpzRegex):
        return "Regex"
    if isinstance(value, TpzToml):
        return "TOMLValue"
    if isinstance(value, TpzUrl):
        return "URL"
    if isinstance(value, TpzTemplate):
        return "template"
    if _is_topaz_enum(value):
        return "enum"
    if _is_topaz_newtype(value):
        return "newtype"
    if _is_topaz_record(value):
        return "record"
    return "unknown"


def tpz_impossible_match(value: object, span: tuple[int, int, int]) -> object:
    tpz_fault("TPZ5001", "non-exhaustive match reached generated fallback", span)


def tpz_return(value: object) -> object:
    raise TpzReturn(value)


def tpz_record_field(value: object, py_field: object, source_field: object, span: tuple[int, int, int]) -> object:
    if not isinstance(py_field, str) or not isinstance(source_field, str):
        tpz_fault("TPZ5001", "record field metadata is malformed", span)
    if not hasattr(value, py_field):
        record_id = getattr(value, "__topaz_record_id__", None)
        if isinstance(record_id, str):
            tpz_fault("TPZ5006", "record `" + record_id + "` has no field `" + source_field + "`", span)
        tpz_fault("TPZ5001", "record has no field `" + source_field + "`", span)
    return getattr(value, py_field)


def _is_topaz_record(value: object) -> bool:
    return isinstance(getattr(value, "__topaz_record_fields__", None), tuple)


def _is_topaz_nominal_record(value: object) -> bool:
    return _is_topaz_record(value) and isinstance(getattr(value, "__topaz_record_id__", None), str)


def _is_topaz_newtype(value: object) -> bool:
    return isinstance(value, TpzNewtype)


def _is_topaz_enum(value: object) -> bool:
    return isinstance(value, TpzEnum)


def tpz_newtype(
    newtype_id: object,
    value: object,
    span: tuple[int, int, int],
    method_identity: object = None,
    declaration_identity: object = None,
) -> TpzNewtype:
    if not isinstance(newtype_id, str):
        tpz_fault("TPZ5001", "newtype metadata is malformed", span)
    if method_identity is not None and not isinstance(method_identity, str):
        tpz_fault("TPZ5001", "newtype method metadata is malformed", span)
    if declaration_identity is not None and not isinstance(declaration_identity, str):
        tpz_fault("TPZ5001", "newtype declaration metadata is malformed", span)
    return TpzNewtype(newtype_id, value, method_identity, declaration_identity)


def tpz_is_newtype(value: object, newtype_id: object) -> bool:
    return (
        isinstance(newtype_id, str)
        and _is_topaz_newtype(value)
        and _tpz_nominal_declaration_identity(value) == newtype_id
    )


def tpz_is_nominal_record(value: object, record_id: object) -> bool:
    return (
        isinstance(record_id, str)
        and _is_topaz_nominal_record(value)
        and _tpz_nominal_declaration_identity(value) == record_id
    )


def tpz_newtype_unwrap(value: object, span: tuple[int, int, int]) -> object:
    if _is_topaz_newtype(value):
        return value.value
    tpz_fault("TPZ5001", "`.value()` needs a newtype, found a " + tpz_kind(value), span)


def tpz_enum(
    enum_id: object,
    variant: object,
    variant_index: object,
    payloads: object,
    span: tuple[int, int, int],
    method_identity: object = None,
    declaration_identity: object = None,
) -> TpzEnum:
    if not isinstance(enum_id, str) or not isinstance(variant, str) or type(variant_index) is not int:
        tpz_fault("TPZ5001", "enum metadata is malformed", span)
    if not isinstance(payloads, list):
        tpz_fault("TPZ5001", "enum payload metadata is malformed", span)
    if method_identity is not None and not isinstance(method_identity, str):
        tpz_fault("TPZ5001", "enum method metadata is malformed", span)
    if declaration_identity is not None and not isinstance(declaration_identity, str):
        tpz_fault("TPZ5001", "enum declaration metadata is malformed", span)
    return TpzEnum(
        enum_id,
        variant,
        variant_index,
        tuple(payloads),
        method_identity,
        declaration_identity,
    )


def tpz_is_enum(value: object, enum_ids: object, variant: object, arity: object) -> bool:
    if not _is_topaz_enum(value) or not isinstance(enum_ids, tuple) or not isinstance(variant, str):
        return False
    if _tpz_nominal_declaration_identity(value) not in enum_ids or value.variant != variant:
        return False
    return arity is None or len(value.payloads) == arity


def tpz_enum_bare_variant_matches(value: object, enum_ids: object, variant: object) -> bool:
    if not isinstance(enum_ids, tuple) or not isinstance(variant, str):
        return False
    if _is_topaz_enum(value) and _tpz_nominal_declaration_identity(value) in enum_ids:
        return value.variant == variant
    return True


def tpz_enum_bare_variant_binds(value: object, enum_ids: object) -> bool:
    return not (
        _is_topaz_enum(value)
        and isinstance(enum_ids, tuple)
        and _tpz_nominal_declaration_identity(value) in enum_ids
    )


def tpz_enum_pattern(
    value: object,
    enum_ids: object,
    variant: object,
    arity: object,
    span: tuple[int, int, int],
) -> bool:
    if not isinstance(arity, int):
        tpz_fault("TPZ5001", "enum pattern metadata is malformed", span)
    if not _is_topaz_enum(value) or not isinstance(enum_ids, tuple) or not isinstance(variant, str):
        return False
    if _tpz_nominal_declaration_identity(value) not in enum_ids or value.variant != variant:
        return False
    if len(value.payloads) != arity:
        tpz_fault(
            "TPZ5001",
            "enum variant `" + variant + "` pattern takes "
            + str(len(value.payloads))
            + " subpattern"
            + ("" if len(value.payloads) == 1 else "s"),
            span,
        )
    return True


def tpz_type_matches(value: object, spec: object) -> bool:
    return _tpz_type_matches(value, spec, {})


def _tpz_python_callable_arity(value: object, skip_first: int = 0) -> tuple[int, int | None] | None:
    try:
        signature = inspect.signature(value)
    except (TypeError, ValueError):
        return (0, None) if callable(value) else None
    required = 0
    fixed = 0
    variadic = False
    for param in signature.parameters.values():
        if param.kind in (
            inspect.Parameter.POSITIONAL_ONLY,
            inspect.Parameter.POSITIONAL_OR_KEYWORD,
        ):
            fixed += 1
            if param.default is inspect.Parameter.empty:
                required += 1
        elif param.kind is inspect.Parameter.VAR_POSITIONAL:
            variadic = True
    if skip_first:
        skipped = min(skip_first, fixed)
        fixed -= skipped
        required = max(0, required - skipped)
    return required, None if variadic else fixed


def _tpz_callable_arity(value: object) -> tuple[int, int | None] | None:
    if isinstance(value, TpzComposed):
        return _tpz_callable_arity(value.first)
    if isinstance(value, TpzHostCallable):
        minimum, maximum = _tpz_python_callable_arity(value.fn, 1)
        if value.variadic_py_name is not None:
            return minimum, None
        return minimum, maximum
    if isinstance(value, TpzExternFunction):
        return (0, None)
    return _tpz_python_callable_arity(value)


def _tpz_callable_shape_matches(value: object, n_fixed: int, type_variadic: bool) -> bool:
    arity = _tpz_callable_arity(value)
    if arity is None:
        return False
    minimum, maximum = arity
    if type_variadic:
        return maximum is None and minimum <= n_fixed
    return minimum <= n_fixed and (maximum is None or n_fixed <= maximum)


def _tpz_literal_text_matches(value: object, text: str) -> bool:
    if value is TPZ_NULL:
        return text == "null"
    if type(value) is bool:
        return text == ("true" if value else "false")
    if type(value) is int:
        try:
            return int(text) == value
        except ValueError:
            return False
    if type(value) is float:
        try:
            return float(text) == value
        except ValueError:
            return False
    if isinstance(value, str):
        return (
            len(text) >= 2
            and text[0] == "\""
            and text[-1] == "\""
            and text[1:-1] == value
        )
    return False


def _tpz_type_matches(value: object, spec: object, refs: dict[str, object]) -> bool:
    if spec == "int":
        return type(value) is int
    if spec == "float":
        return type(value) is float
    if spec == "string":
        return isinstance(value, str)
    if spec == "bool":
        return type(value) is bool
    if spec == "unit":
        return value is TPZ_UNIT
    if spec == "JSONValue":
        return isinstance(value, TpzJson)
    if spec == "Bytes":
        return isinstance(value, TpzBytes)
    if spec == "ByteBuffer":
        return isinstance(value, TpzByteBuffer)
    if not isinstance(spec, tuple) or len(spec) == 0:
        return False

    tag = spec[0]
    if tag == "literal":
        return (
            len(spec) == 2
            and isinstance(spec[1], str)
            and _tpz_literal_text_matches(value, spec[1])
        )
    if tag == "type_ref":
        if len(spec) != 2 or not isinstance(spec[1], str):
            return False
        target = refs.get(spec[1])
        return target is not None and _tpz_type_matches(value, target, refs)
    if tag == "union":
        return any(_tpz_type_matches(value, member, refs) for member in spec[1])
    if tag == "option":
        return value is None or (
            isinstance(value, Some) and _tpz_type_matches(value.value, spec[1], refs)
        )
    if tag == "result":
        return (
            isinstance(value, Ok)
            and _tpz_type_matches(value.value, spec[1], refs)
        ) or (
            isinstance(value, Err)
            and _tpz_type_matches(value.value, spec[2], refs)
        )
    if tag == "array":
        return isinstance(value, list) and all(_tpz_type_matches(item, spec[1], refs) for item in value)
    if tag == "set":
        return isinstance(value, TpzSet) and all(
            _tpz_type_matches(_key_to_value(item), spec[1], refs) for item in value.items
        )
    if tag == "map":
        return isinstance(value, TpzMap) and all(
            _tpz_type_matches(_key_to_value(key), spec[1], refs) and _tpz_type_matches(item, spec[2], refs)
            for key, item in value.entries
        )
    if tag == "function":
        return (
            len(spec) == 3
            and isinstance(spec[1], int)
            and type(spec[2]) is bool
            and _tpz_callable_shape_matches(value, spec[1], spec[2])
        )
    if tag == "record":
        if not _is_topaz_record(value) or _is_topaz_nominal_record(value):
            return False
        fields = spec[1]
        actual = getattr(value, "__topaz_record_fields__", ())
        if len(actual) != len(fields):
            return False
        actual_by_source = {source: py for py, source in actual}
        for source_field, py_field, field_spec in fields:
            if actual_by_source.get(source_field) != py_field:
                return False
            if not _tpz_type_matches(getattr(value, py_field), field_spec, refs):
                return False
        return True
    if tag == "nominal_record":
        if (
            not _is_topaz_nominal_record(value)
            or _tpz_nominal_declaration_identity(value) != spec[1]
        ):
            return False
        if len(spec) == 2:
            return True
        fields = spec[2]
        actual = getattr(value, "__topaz_record_fields__", ())
        actual_by_source = {source: py for py, source in actual}
        for source_field, py_field, field_spec in fields:
            if actual_by_source.get(source_field) != py_field:
                return False
            if not hasattr(value, py_field):
                return False
            if not _tpz_type_matches(getattr(value, py_field), field_spec, refs):
                return False
        return True
    if tag == "newtype":
        if not _is_topaz_newtype(value) or _tpz_nominal_declaration_identity(value) != spec[1]:
            return False
        if len(spec) == 2:
            return True
        if len(spec) != 4 or not isinstance(spec[2], str):
            return False
        ref_key = spec[2]
        old_ref = refs.get(ref_key)
        refs[ref_key] = spec
        try:
            return _tpz_type_matches(value.value, spec[3], refs)
        finally:
            if old_ref is None:
                refs.pop(ref_key, None)
            else:
                refs[ref_key] = old_ref
    if tag == "enum":
        if not _is_topaz_enum(value) or _tpz_nominal_declaration_identity(value) != spec[1]:
            return False
        if len(spec) == 2:
            return True
        if len(spec) == 3:
            variants = spec[2]
            ref_key = None
        elif len(spec) == 4:
            if not isinstance(spec[2], str):
                return False
            ref_key = spec[2]
            variants = spec[3]
        else:
            return False
        if not isinstance(variants, tuple):
            return False
        old_ref = refs.get(ref_key) if ref_key is not None else None
        if ref_key is not None:
            refs[ref_key] = spec
        for variant_spec in variants:
            if not isinstance(variant_spec, tuple) or len(variant_spec) != 2:
                if ref_key is not None:
                    if old_ref is None:
                        refs.pop(ref_key, None)
                    else:
                        refs[ref_key] = old_ref
                return False
            variant_name, payload_specs = variant_spec
            if variant_name != value.variant:
                continue
            if not isinstance(payload_specs, tuple) or len(payload_specs) != len(value.payloads):
                if ref_key is not None:
                    if old_ref is None:
                        refs.pop(ref_key, None)
                    else:
                        refs[ref_key] = old_ref
                return False
            matched = all(
                _tpz_type_matches(payload, payload_spec, refs)
                for payload, payload_spec in zip(value.payloads, payload_specs)
            )
            if ref_key is not None:
                if old_ref is None:
                    refs.pop(ref_key, None)
                else:
                    refs[ref_key] = old_ref
            return matched
        if ref_key is not None:
            if old_ref is None:
                refs.pop(ref_key, None)
            else:
                refs[ref_key] = old_ref
        return False
    return False


def _record_class_for_fields(
    fields: tuple[tuple[str, str], ...],
    record_id: str | None = None,
    declaration_identity: str | None = None,
) -> type:
    key = (record_id, declaration_identity, fields)
    cached = _RECORD_CLASS_CACHE.get(key)
    if cached is not None:
        return cached
    namespace: dict[str, object] = {"__topaz_record_fields__": fields}
    if record_id is not None:
        namespace["__topaz_record_id__"] = record_id
    if declaration_identity is not None:
        namespace["__topaz_declaration_identity__"] = declaration_identity
    cls = make_dataclass(
        "TopazRecord" + str(len(_RECORD_CLASS_CACHE)),
        [(py_field, object) for py_field, _ in fields],
        frozen=True,
        slots=True,
        namespace=namespace,
    )
    _RECORD_CLASS_CACHE[key] = cls
    return cls


def _tpz_validate_nominal_record_metadata(
    record_id: str,
    decl_fields: list[tuple[str, str, object]],
    fields: list[tuple[str, str, object]],
    span: tuple[int, int, int],
) -> None:
    decl_by_py: dict[str, tuple[str, object]] = {}
    for py_field, source_field, default in decl_fields:
        if not isinstance(py_field, str) or not isinstance(source_field, str):
            tpz_fault("TPZ5001", "nominal record metadata is malformed", span)
        decl_by_py[py_field] = (source_field, default)
    seen: set[str] = set()
    for py_field, source_field, _thunk in fields:
        if not isinstance(py_field, str) or not isinstance(source_field, str):
            tpz_fault("TPZ5001", "nominal record field metadata is malformed", span)
        if py_field not in decl_by_py:
            tpz_fault("TPZ5006", "record `" + record_id + "` has no field `" + source_field + "`", span)
        if py_field in seen:
            tpz_fault("TPZ5004", "field `" + source_field + "` is given twice in `" + record_id + "`", span)
        seen.add(py_field)


def tpz_nominal_record(
    record_type: type,
    record_id: str,
    decl_fields: list[tuple[str, str, object]],
    spread,
    fields: list[tuple[str, str, object]],
    span: tuple[int, int, int],
    declaration_identity: str | None = None,
) -> object:
    _tpz_validate_nominal_record_metadata(record_id, decl_fields, fields, span)
    values: dict[str, object] = {}
    if spread is not None:
        base = spread()
        expected_identity = _tpz_effective_nominal_declaration_identity(
            record_id, declaration_identity
        )
        if (
            _is_topaz_nominal_record(base)
            and _tpz_nominal_declaration_identity(base) != expected_identity
        ):
            tpz_fault(
                "TPZ5001",
                "record spread `...` needs a `" + record_id + "`, found a `" + base.__topaz_record_id__ + "`",
                span,
            )
        if not _is_topaz_nominal_record(base):
            tpz_fault(
                "TPZ5001",
                "record spread `...` needs a `" + record_id + "`, found a " + tpz_kind(base),
                span,
            )
        for py_field, _source_field, _default in decl_fields:
            values[py_field] = getattr(base, py_field)

    for py_field, _source_field, thunk in fields:
        values[py_field] = thunk()

    for py_field, source_field, default in decl_fields:
        if py_field in values:
            continue
        if default is None:
            tpz_fault("TPZ5004", "record `" + record_id + "` is missing field `" + source_field + "`", span)
        values[py_field] = default()
    return record_type(**values)


def tpz_nominal_record__co(
    record_type: type,
    record_id: str,
    decl_fields: list[tuple[str, str, object]],
    spread,
    fields: list[tuple[str, str, object]],
    span: tuple[int, int, int],
    declaration_identity: str | None = None,
) -> object:
    _tpz_validate_nominal_record_metadata(record_id, decl_fields, fields, span)
    values: dict[str, object] = {}
    if spread is not None:
        base = yield from spread()
        expected_identity = _tpz_effective_nominal_declaration_identity(
            record_id, declaration_identity
        )
        if (
            _is_topaz_nominal_record(base)
            and _tpz_nominal_declaration_identity(base) != expected_identity
        ):
            tpz_fault(
                "TPZ5001",
                "record spread `...` needs a `" + record_id + "`, found a `" + base.__topaz_record_id__ + "`",
                span,
            )
        if not _is_topaz_nominal_record(base):
            tpz_fault(
                "TPZ5001",
                "record spread `...` needs a `" + record_id + "`, found a " + tpz_kind(base),
                span,
            )
        for py_field, _source_field, _default in decl_fields:
            values[py_field] = getattr(base, py_field)

    for py_field, _source_field, thunk in fields:
        values[py_field] = yield from thunk()

    for py_field, source_field, default in decl_fields:
        if py_field in values:
            continue
        if default is None:
            tpz_fault("TPZ5004", "record `" + record_id + "` is missing field `" + source_field + "`", span)
        values[py_field] = yield from tpz_call_cooperative(default, (), {}, span)
    return record_type(**values)


def tpz_record_update(base: object, fields: list[tuple[str, str, object]], span: tuple[int, int, int]) -> object:
    if not _is_topaz_record(base):
        tpz_fault("TPZ5001", "record update needs a record, found `" + tpz_kind(base) + "`", span)
    values = {py_field: getattr(base, py_field) for py_field, _ in base.__topaz_record_fields__}
    field_names = {py_field: source_field for py_field, source_field in base.__topaz_record_fields__}
    evaluated: list[tuple[str, str, object]] = []
    for py_field, source_field, thunk in fields:
        if not isinstance(py_field, str) or not isinstance(source_field, str):
            tpz_fault("TPZ5001", "record update metadata is malformed", span)
        evaluated.append((py_field, source_field, thunk()))
    updating = len(values)
    for py_field, source_field, value in evaluated:
        if updating > 0 and py_field not in values:
            tpz_fault("TPZ5006", "record update names unknown field `" + source_field + "`", span)
        values[py_field] = value
        field_names[py_field] = source_field
    if updating == 0 and values:
        metadata = tuple((py_field, field_names[py_field]) for py_field in values)
        return _record_class_for_fields(metadata)(**values)
    return type(base)(**values)


def _int(value: object, span: tuple[int, int, int]) -> int:
    if type(value) is not int:
        tpz_fault("TPZ5001", "expected `int`", span)
    return value


def tpz_range(
    lo: object,
    hi: object,
    inclusive: bool,
    step: object | None,
    span: tuple[int, int, int],
) -> TpzRange:
    if type(lo) is not int or type(hi) is not int:
        tpz_fault("TPZ5001", "range endpoints must be `int` in this build (§10)", span)
    if step is None:
        step_value = 1
    elif type(step) is int:
        if step == 0:
            tpz_fault("TPZ4003", "range step must not be zero (§10)", span)
        step_value = step
    else:
        tpz_fault(
            "TPZ5001",
            "range step must be `int`, found `" + tpz_kind(step) + "`",
            span,
        )
    return TpzRange(lo, hi, inclusive, step_value)


def tpz_make_template(tag: object, parts: object, values: object) -> TpzTemplate:
    if not isinstance(tag, str):
        tpz_fault("TPZ5001", "template tag metadata is malformed", (0, 0, 0))
    if not isinstance(parts, list) or not all(isinstance(part, str) for part in parts):
        tpz_fault("TPZ5001", "template parts metadata is malformed", (0, 0, 0))
    if not isinstance(values, list):
        tpz_fault("TPZ5001", "template interpolation metadata is malformed", (0, 0, 0))
    frozen_parts = tuple(parts)
    frozen_values = tuple(values)
    normalized = ""
    if tag == "p":
        assembled = []
        for index, part in enumerate(frozen_parts):
            assembled.append(part)
            if index < len(frozen_values):
                assembled.append(tpz_render(frozen_values[index]))
        normalized = "".join(assembled).replace("\\", "/")
    return TpzTemplate(tag, frozen_parts, frozen_values, normalized)


def _check_i64(value: int, span: tuple[int, int, int], message: str) -> int:
    if value < INT_MIN or value > INT_MAX:
        tpz_fault("TPZ4004", message, span)
    return value


def tpz_i64(value: object, span: tuple[int, int, int]) -> int:
    return _check_i64(_int(value, span), span, "integer value is outside i64 range")


def tpz_add_i64(a: object, b: object, span: tuple[int, int, int]) -> int:
    return _check_i64(_int(a, span) + _int(b, span), span, "integer addition overflows")


def tpz_sub_i64(a: object, b: object, span: tuple[int, int, int]) -> int:
    return _check_i64(_int(a, span) - _int(b, span), span, "integer subtraction overflows")


def tpz_mul_i64(a: object, b: object, span: tuple[int, int, int]) -> int:
    return _check_i64(_int(a, span) * _int(b, span), span, "integer multiplication overflows")


def tpz_div_trunc_i64(a: object, b: object, span: tuple[int, int, int]) -> int:
    lhs = _int(a, span)
    rhs = _int(b, span)
    if rhs == 0:
        tpz_fault("TPZ4002", "integer division by zero", span)
    if lhs == INT_MIN and rhs == -1:
        tpz_fault("TPZ4004", "integer division overflows", span)
    q = abs(lhs) // abs(rhs)
    if (lhs < 0) != (rhs < 0):
        q = -q
    return _check_i64(q, span, "integer division overflows")


def tpz_rem_trunc_i64(a: object, b: object, span: tuple[int, int, int]) -> int:
    lhs = _int(a, span)
    rhs = _int(b, span)
    if rhs == 0:
        tpz_fault("TPZ4002", "integer remainder by zero", span)
    if lhs == INT_MIN and rhs == -1:
        tpz_fault("TPZ4004", "integer remainder overflows", span)
    return _check_i64(
        lhs - tpz_div_trunc_i64(lhs, rhs, span) * rhs,
        span,
        "integer remainder overflows",
    )


def tpz_pow_i64(a: object, b: object, span: tuple[int, int, int]) -> int:
    base = _int(a, span)
    exp = _int(b, span)
    if exp < 0:
        tpz_fault("TPZ4005", "integer exponent must be non-negative; use float operands", span)
    if exp > U32_MAX:
        tpz_fault("TPZ4004", "integer exponentiation overflows", span)
    result = 1
    factor = base
    n = exp
    while n > 0:
        if n & 1:
            result = _check_i64(result * factor, span, "integer exponentiation overflows")
        n >>= 1
        if n > 0:
            factor = _check_i64(factor * factor, span, "integer exponentiation overflows")
    return result


def _numeric_kind_fault(span: tuple[int, int, int]) -> None:
    tpz_fault("TPZ5001", "numeric operands must both be `int` or both be `float`", span)


def _float_div(a: float, b: float) -> float:
    if b == 0.0:
        if a == 0.0 or math.isnan(a) or math.isnan(b):
            return _CANONICAL_ARITHMETIC_NAN
        sign = math.copysign(1.0, a) * math.copysign(1.0, b)
        return math.copysign(math.inf, sign)
    return _canonicalize_arithmetic_nan(a / b)


def _float_pow(a: float, b: float) -> float:
    if a == 0.0 and b < 0.0:
        negative_sign = math.copysign(1.0, a) < 0.0 and b.is_integer() and int(abs(b)) % 2 == 1
        return math.copysign(math.inf, -1.0 if negative_sign else 1.0)
    if math.isfinite(a) and a < 0.0 and math.isfinite(b) and not b.is_integer():
        return _CANONICAL_ARITHMETIC_NAN
    try:
        result = a**b
    except ValueError:
        return _CANONICAL_ARITHMETIC_NAN
    except ZeroDivisionError:
        if a == 0.0 and b < 0.0:
            negative_sign = math.copysign(1.0, a) < 0.0 and b.is_integer() and int(abs(b)) % 2 == 1
            return math.copysign(math.inf, -1.0 if negative_sign else 1.0)
        return _CANONICAL_ARITHMETIC_NAN
    except OverflowError:
        sign = -1.0 if a < 0.0 and b.is_integer() and int(abs(b)) % 2 == 1 else 1.0
        return math.copysign(math.inf, sign)
    if type(result) is complex:
        return _CANONICAL_ARITHMETIC_NAN
    return _canonicalize_arithmetic_nan(result)


def tpz_add(a: object, b: object, span: tuple[int, int, int]) -> int | float | str:
    if type(a) is int and type(b) is int:
        return tpz_add_i64(a, b, span)
    if type(a) is float and type(b) is float:
        return _canonicalize_arithmetic_nan(a + b)
    if type(a) is str and type(b) is str:
        return a + b
    _numeric_kind_fault(span)


def tpz_sub(a: object, b: object, span: tuple[int, int, int]) -> int | float:
    if type(a) is int and type(b) is int:
        return tpz_sub_i64(a, b, span)
    if type(a) is float and type(b) is float:
        return _canonicalize_arithmetic_nan(a - b)
    _numeric_kind_fault(span)


def tpz_mul(a: object, b: object, span: tuple[int, int, int]) -> int | float:
    if type(a) is int and type(b) is int:
        return tpz_mul_i64(a, b, span)
    if type(a) is float and type(b) is float:
        return _canonicalize_arithmetic_nan(a * b)
    _numeric_kind_fault(span)


def tpz_div(a: object, b: object, span: tuple[int, int, int]) -> int | float:
    if type(a) is int and type(b) is int:
        return tpz_div_trunc_i64(a, b, span)
    if type(a) is float and type(b) is float:
        return _float_div(a, b)
    _numeric_kind_fault(span)


def tpz_pow(a: object, b: object, span: tuple[int, int, int]) -> int | float:
    if type(a) is int and type(b) is int:
        return tpz_pow_i64(a, b, span)
    if type(a) is float and type(b) is float:
        return _float_pow(a, b)
    _numeric_kind_fault(span)


def tpz_neg(value: object, span: tuple[int, int, int]) -> int | float:
    if type(value) is int:
        return tpz_sub_i64(0, value, span)
    if type(value) is float:
        return -value
    tpz_fault("TPZ5001", "numeric operand must be `int` or `float`", span)


def _non_comparable_kind(value: object) -> str | None:
    if isinstance(value, TpzMap):
        return "Map"
    if isinstance(value, TpzSet):
        return "Set"
    if isinstance(value, TpzFile):
        return "File"
    if isinstance(value, TpzJson):
        return "JSONValue"
    if isinstance(value, TpzByteBuffer):
        return "ByteBuffer"
    if isinstance(value, TpzRange):
        return "range"
    if isinstance(value, (TpzComposed, TpzHostCallable, TpzExternFunction)) or callable(value):
        return "function"
    return None


def _cmp_guard_not_comparable(kind: str, span: tuple[int, int, int]) -> None:
    tpz_fault("TPZ5007", "`" + kind + "` values are not comparable", span)


def _tpz_values_equal(
    a: object,
    b: object,
    span: tuple[int, int, int],
    budget: _StructBudget | None = None,
    depth: int = 0,
) -> bool:
    if budget is None:
        budget = _StructBudget()
    budget.consume(depth, span)
    kind = _non_comparable_kind(a) or _non_comparable_kind(b)
    if kind is not None:
        _cmp_guard_not_comparable(kind, span)

    if type(a) is bool and type(b) is bool:
        return a == b
    if type(a) is int and type(b) is int:
        return a == b
    if type(a) is float and type(b) is float:
        return a == b
    if isinstance(a, str) and isinstance(b, str):
        return a == b
    if isinstance(a, TpzBytes) and isinstance(b, TpzBytes):
        return a.data == b.data
    if a is TPZ_UNIT and b is TPZ_UNIT:
        return True
    if a is TPZ_NULL and b is TPZ_NULL:
        return True
    if a is None and b is None:
        return True
    if isinstance(a, Some) and isinstance(b, Some):
        return _tpz_values_equal(a.value, b.value, span, budget, depth + 1)
    if isinstance(a, Ok) and isinstance(b, Ok):
        return _tpz_values_equal(a.value, b.value, span, budget, depth + 1)
    if isinstance(a, Err) and isinstance(b, Err):
        return _tpz_values_equal(a.value, b.value, span, budget, depth + 1)
    if isinstance(a, list) and isinstance(b, list):
        if len(a) != len(b):
            return False
        for left, right in zip(a, b):
            if not _tpz_values_equal(left, right, span, budget, depth + 1):
                return False
        return True
    if _is_topaz_newtype(a) and _is_topaz_newtype(b):
        if _tpz_nominal_declaration_identity(a) != _tpz_nominal_declaration_identity(b):
            return False
        return _tpz_values_equal(a.value, b.value, span, budget, depth + 1)
    if _is_topaz_newtype(a) or _is_topaz_newtype(b):
        return False
    if _is_topaz_enum(a) and _is_topaz_enum(b):
        if (
            _tpz_nominal_declaration_identity(a)
            != _tpz_nominal_declaration_identity(b)
            or a.variant != b.variant
        ):
            return False
        if len(a.payloads) != len(b.payloads):
            return False
        for left, right in zip(a.payloads, b.payloads):
            if not _tpz_values_equal(left, right, span, budget, depth + 1):
                return False
        return True
    if _is_topaz_enum(a) or _is_topaz_enum(b):
        return False
    if _is_topaz_nominal_record(a) and _is_topaz_nominal_record(b):
        if _tpz_nominal_declaration_identity(a) != _tpz_nominal_declaration_identity(b):
            return False
        if len(a.__topaz_record_fields__) != len(b.__topaz_record_fields__):
            return False
        for (left_field, _), (right_field, _) in zip(a.__topaz_record_fields__, b.__topaz_record_fields__):
            if not _tpz_values_equal(
                getattr(a, left_field),
                getattr(b, right_field),
                span,
                budget,
                depth + 1,
            ):
                return False
        return True
    if _is_topaz_record(a) and _is_topaz_record(b):
        left_fields = sorted(a.__topaz_record_fields__, key=lambda item: item[1])
        right_fields = sorted(b.__topaz_record_fields__, key=lambda item: item[1])
        if [source for _, source in left_fields] != [source for _, source in right_fields]:
            tpz_fault("TPZ5007", "records with different field sets are not comparable", span)
        for (left_field, _), (right_field, _) in zip(left_fields, right_fields):
            if not _tpz_values_equal(
                getattr(a, left_field),
                getattr(b, right_field),
                span,
                budget,
                depth + 1,
            ):
                return False
        return True
    return False


def tpz_eq(a: object, b: object, span: tuple[int, int, int]) -> bool:
    return _tpz_values_equal(a, b, span)


def tpz_ne(a: object, b: object, span: tuple[int, int, int]) -> bool:
    return not _tpz_values_equal(a, b, span)


def _newtype_order_operands(a: object, b: object, span: tuple[int, int, int]) -> tuple[object, object]:
    if (
        _is_topaz_newtype(a)
        and _is_topaz_newtype(b)
        and _tpz_nominal_declaration_identity(a)
        == _tpz_nominal_declaration_identity(b)
    ):
        return a.value, b.value
    tpz_fault("TPZ5007", "`newtype` values are not comparable", span)


def _tpz_order_compare(
    a: object,
    b: object,
    span: tuple[int, int, int],
    budget: _StructBudget | None = None,
    depth: int = 0,
) -> int:
    if budget is None:
        budget = _StructBudget()
    budget.consume(depth, span)
    if _is_topaz_newtype(a) or _is_topaz_newtype(b):
        left, right = _newtype_order_operands(a, b, span)
        return _tpz_order_compare(left, right, span, budget, depth + 1)
    if _is_topaz_enum(a) or _is_topaz_enum(b):
        if not (_is_topaz_enum(a) and _is_topaz_enum(b)):
            _cmp_guard_not_comparable("enum", span)
        if a.enum_id == "RoundingMode" or b.enum_id == "RoundingMode":
            _cmp_guard_not_comparable("RoundingMode", span)
        if _tpz_nominal_declaration_identity(a) != _tpz_nominal_declaration_identity(b):
            _cmp_guard_not_comparable("enum", span)
        if a.variant_index != b.variant_index:
            return -1 if a.variant_index < b.variant_index else 1
        for left, right in zip(a.payloads, b.payloads):
            cmp = _tpz_order_compare(left, right, span, budget, depth + 1)
            if cmp != 0:
                return cmp
        if len(a.payloads) == len(b.payloads):
            return 0
        return -1 if len(a.payloads) < len(b.payloads) else 1
    if _is_topaz_nominal_record(a) or _is_topaz_nominal_record(b):
        if not (_is_topaz_nominal_record(a) and _is_topaz_nominal_record(b)):
            _cmp_guard_not_comparable("record", span)
        if _tpz_nominal_declaration_identity(a) != _tpz_nominal_declaration_identity(b):
            _cmp_guard_not_comparable("record", span)
        for (left_field, _), (right_field, _) in zip(
            a.__topaz_record_fields__, b.__topaz_record_fields__
        ):
            cmp = _tpz_order_compare(
                getattr(a, left_field),
                getattr(b, right_field),
                span,
                budget,
                depth + 1,
            )
            if cmp != 0:
                return cmp
        if len(a.__topaz_record_fields__) == len(b.__topaz_record_fields__):
            return 0
        return -1 if len(a.__topaz_record_fields__) < len(b.__topaz_record_fields__) else 1
    if type(a) is int and type(b) is int:
        return -1 if a < b else (1 if a > b else 0)
    if type(a) is float and type(b) is float:
        return -1 if a < b else (1 if a > b else 0)
    if type(a) is str and type(b) is str:
        return -1 if a < b else (1 if a > b else 0)
    if isinstance(a, TpzBytes) and isinstance(b, TpzBytes):
        return -1 if a.data < b.data else (1 if a.data > b.data else 0)
    _numeric_kind_fault(span)


def tpz_lt(a: object, b: object, span: tuple[int, int, int]) -> bool:
    if _is_topaz_enum(a) or _is_topaz_enum(b):
        return _tpz_order_compare(a, b, span) < 0
    if _is_topaz_newtype(a) or _is_topaz_newtype(b):
        left, right = _newtype_order_operands(a, b, span)
        return tpz_lt(left, right, span)
    if type(a) is int and type(b) is int:
        return tpz_lt_i64(a, b, span)
    if type(a) is float and type(b) is float:
        return a < b
    if type(a) is str and type(b) is str:
        return a < b
    if isinstance(a, TpzBytes) and isinstance(b, TpzBytes):
        return a.data < b.data
    _numeric_kind_fault(span)


def tpz_le(a: object, b: object, span: tuple[int, int, int]) -> bool:
    if _is_topaz_enum(a) or _is_topaz_enum(b):
        return _tpz_order_compare(a, b, span) <= 0
    if _is_topaz_newtype(a) or _is_topaz_newtype(b):
        left, right = _newtype_order_operands(a, b, span)
        return tpz_le(left, right, span)
    if type(a) is int and type(b) is int:
        return tpz_le_i64(a, b, span)
    if type(a) is float and type(b) is float:
        return a <= b
    if type(a) is str and type(b) is str:
        return a <= b
    if isinstance(a, TpzBytes) and isinstance(b, TpzBytes):
        return a.data <= b.data
    _numeric_kind_fault(span)


def tpz_gt(a: object, b: object, span: tuple[int, int, int]) -> bool:
    if _is_topaz_enum(a) or _is_topaz_enum(b):
        return _tpz_order_compare(a, b, span) > 0
    if _is_topaz_newtype(a) or _is_topaz_newtype(b):
        left, right = _newtype_order_operands(a, b, span)
        return tpz_gt(left, right, span)
    if type(a) is int and type(b) is int:
        return tpz_gt_i64(a, b, span)
    if type(a) is float and type(b) is float:
        return a > b
    if type(a) is str and type(b) is str:
        return a > b
    if isinstance(a, TpzBytes) and isinstance(b, TpzBytes):
        return a.data > b.data
    _numeric_kind_fault(span)


def tpz_ge(a: object, b: object, span: tuple[int, int, int]) -> bool:
    if _is_topaz_enum(a) or _is_topaz_enum(b):
        return _tpz_order_compare(a, b, span) >= 0
    if _is_topaz_newtype(a) or _is_topaz_newtype(b):
        left, right = _newtype_order_operands(a, b, span)
        return tpz_ge(left, right, span)
    if type(a) is int and type(b) is int:
        return tpz_ge_i64(a, b, span)
    if type(a) is float and type(b) is float:
        return a >= b
    if type(a) is str and type(b) is str:
        return a >= b
    if isinstance(a, TpzBytes) and isinstance(b, TpzBytes):
        return a.data >= b.data
    _numeric_kind_fault(span)


def tpz_lt_i64(a: object, b: object, span: tuple[int, int, int]) -> bool:
    return _int(a, span) < _int(b, span)


def tpz_le_i64(a: object, b: object, span: tuple[int, int, int]) -> bool:
    return _int(a, span) <= _int(b, span)


def tpz_gt_i64(a: object, b: object, span: tuple[int, int, int]) -> bool:
    return _int(a, span) > _int(b, span)


def tpz_ge_i64(a: object, b: object, span: tuple[int, int, int]) -> bool:
    return _int(a, span) >= _int(b, span)


def tpz_condition(value: object, span: tuple[int, int, int]) -> bool:
    if type(value) is not bool:
        tpz_fault("TPZ5001", "condition must be `bool`", span)
    return value


def tpz_coalesce(value: object, rhs) -> object:
    if isinstance(value, Some):
        return value.value
    if value is None or value is TPZ_NULL:
        return rhs()
    return value


def tpz_to_int(value: object, span: tuple[int, int, int]) -> Some | None:
    if not isinstance(value, str):
        tpz_fault("TPZ5001", "`toInt` takes a `string`", span)
    try:
        parsed = int(value.strip(), 10)
    except ValueError:
        return None
    if parsed < INT_MIN or parsed > INT_MAX:
        return None
    return Some(parsed)


def tpz_from_code_point(value: object, span: tuple[int, int, int]) -> Some | None:
    scalar = _int(value, span)
    if scalar < 0 or scalar > 0x10FFFF or 0xD800 <= scalar <= 0xDFFF:
        return None
    return Some(chr(scalar))


def _string_arg(value: object, label: str, span: tuple[int, int, int]) -> str:
    if not isinstance(value, str):
        tpz_fault("TPZ5001", label + " takes a `string`, found `" + tpz_kind(value) + "`", span)
    return value


def tpz_string_starts_with(value: object, prefix: object, span: tuple[int, int, int]) -> bool:
    if not isinstance(value, str):
        tpz_fault("TPZ5001", "`str.startsWith` takes a string receiver", span)
    return value.startswith(_string_arg(prefix, "`str.startsWith`", span))


def tpz_string_ends_with(value: object, suffix: object, span: tuple[int, int, int]) -> bool:
    if not isinstance(value, str):
        tpz_fault("TPZ5001", "`str.endsWith` takes a string receiver", span)
    return value.endswith(_string_arg(suffix, "`str.endsWith`", span))


def tpz_string_contains(value: object, sub: object, span: tuple[int, int, int]) -> bool:
    if not isinstance(value, str):
        tpz_fault("TPZ5001", "`str.contains` takes a string receiver", span)
    return _string_arg(sub, "`str.contains`", span) in value


def tpz_string_index_of(value: object, sub: object, span: tuple[int, int, int]) -> Some | None:
    if not isinstance(value, str):
        tpz_fault("TPZ5001", "`str.indexOf` takes a string receiver", span)
    idx = value.find(_string_arg(sub, "`str.indexOf`", span))
    return Some(idx) if idx >= 0 else None


def tpz_string_last_index_of(value: object, sub: object, span: tuple[int, int, int]) -> Some | None:
    if not isinstance(value, str):
        tpz_fault("TPZ5001", "`str.lastIndexOf` takes a string receiver", span)
    idx = value.rfind(_string_arg(sub, "`str.lastIndexOf`", span))
    return Some(idx) if idx >= 0 else None


def tpz_string_trim(value: object, span: tuple[int, int, int]) -> str:
    if not isinstance(value, str):
        tpz_fault("TPZ5001", "`str.trim` takes a string receiver", span)
    return value.strip(" \t\n\r")


def tpz_string_trim_start(value: object, span: tuple[int, int, int]) -> str:
    if not isinstance(value, str):
        tpz_fault("TPZ5001", "`str.trimStart` takes a string receiver", span)
    return value.lstrip(" \t\n\r")


def tpz_string_trim_end(value: object, span: tuple[int, int, int]) -> str:
    if not isinstance(value, str):
        tpz_fault("TPZ5001", "`str.trimEnd` takes a string receiver", span)
    return value.rstrip(" \t\n\r")


def tpz_string_split(value: object, sep: object, span: tuple[int, int, int]) -> list[str]:
    if not isinstance(value, str) or not isinstance(sep, str):
        tpz_fault("TPZ5001", "`split` takes strings", span)
    if sep == "":
        tpz_fault(
            "TPZ5001",
            "`str.split` needs a non-empty separator; use `.scalars()` for a scalar split",
            span,
        )
    return value.split(sep)


def tpz_string_code_point_at(value: object, index: object, span: tuple[int, int, int]) -> Some | None:
    if not isinstance(value, str):
        tpz_fault("TPZ5001", "`codePointAt` takes a string", span)
    idx = _int(index, span)
    if idx < 0 or idx >= len(value):
        return None
    return Some(ord(value[idx]))


def tpz_string_byte_length(value: object, span: tuple[int, int, int]) -> int:
    if not isinstance(value, str):
        tpz_fault("TPZ5001", "`byteLength` takes a string", span)
    return len(value.encode("utf-8"))


def tpz_string_slice(value: object, start: object, end: object, span: tuple[int, int, int]) -> str:
    if not isinstance(value, str):
        tpz_fault("TPZ5001", "`str.slice` takes a string receiver", span)
    st = _int(start, span)
    en = _int(end, span)
    length = len(value)
    st = max(0, min(st, length))
    en = max(st, min(en, length))
    return value[st:en]


def tpz_string_replace(value: object, needle: object, replacement: object, span: tuple[int, int, int]) -> str:
    if not isinstance(value, str):
        tpz_fault("TPZ5001", "`str.replace` takes a string receiver", span)
    return value.replace(
        _string_arg(needle, "`str.replace` needle", span),
        _string_arg(replacement, "`str.replace` replacement", span),
    )


def tpz_array_get(value: object, index: object, span: tuple[int, int, int]) -> Some | None:
    idx = _int(index, span)
    if not isinstance(value, list):
        tpz_fault("TPZ5001", "`arr.get` takes an array receiver", span)
    if idx < 0 or idx >= len(value):
        return None
    return Some(value[idx])


def _array_receiver(value: object, method: str, span: tuple[int, int, int]) -> list[object]:
    if not isinstance(value, list):
        tpz_fault("TPZ5001", "`arr." + method + "` takes an array receiver", span)
    return value


def tpz_array_push(value: object, item: object, span: tuple[int, int, int]) -> object:
    _array_receiver(value, "push", span).append(item)
    return TPZ_UNIT


def tpz_array_slice(value: object, start: object, end: object, span: tuple[int, int, int]) -> list[object]:
    items = _array_receiver(value, "slice", span)
    if type(start) is not int:
        tpz_fault("TPZ5001", "`arr.slice` takes `int` bounds, found `" + tpz_kind(start) + "`", span)
    if type(end) is not int:
        tpz_fault("TPZ5001", "`arr.slice` takes `int` bounds, found `" + tpz_kind(end) + "`", span)
    length = len(items)
    st = max(0, min(start, length))
    en = max(st, min(end, length))
    return list(items[st:en])


def tpz_array_join(value: object, sep: object, span: tuple[int, int, int]) -> str:
    items = _array_receiver(value, "join", span)
    if not isinstance(sep, str):
        tpz_fault("TPZ5001", "`arr.join` takes a `string`, found `" + tpz_kind(sep) + "`", span)
    return sep.join(tpz_render(item) for item in items)


def tpz_array_index_of(value: object, needle: object, span: tuple[int, int, int]) -> Some | None:
    items = _array_receiver(value, "indexOf", span)
    for idx, item in enumerate(items):
        if _tpz_values_equal(item, needle, span):
            return Some(idx)
    return None


def tpz_array_pop(value: object, span: tuple[int, int, int]) -> Some | None:
    items = _array_receiver(value, "pop", span)
    if not items:
        return None
    return Some(items.pop())


def tpz_array_clear(value: object, span: tuple[int, int, int]) -> object:
    _array_receiver(value, "clear", span).clear()
    return TPZ_UNIT


def tpz_array_reverse(value: object, span: tuple[int, int, int]) -> object:
    _array_receiver(value, "reverse", span).reverse()
    return TPZ_UNIT


def tpz_array_insert(value: object, index: object, item: object, span: tuple[int, int, int]) -> object:
    items = _array_receiver(value, "insert", span)
    if type(index) is not int:
        tpz_fault("TPZ5001", "`arr.insert` takes an `int` index, found `" + tpz_kind(index) + "`", span)
    length = len(items)
    if index < 0 or index > length:
        tpz_fault("TPZ4001", "index " + str(index) + " out of bounds for insert into length " + str(length) + " (§6.5)", span)
    items.insert(index, item)
    return TPZ_UNIT


def tpz_array_remove_at(value: object, index: object, span: tuple[int, int, int]) -> Some | None:
    items = _array_receiver(value, "removeAt", span)
    if type(index) is not int:
        tpz_fault("TPZ5001", "`arr.removeAt` takes an `int` index, found `" + tpz_kind(index) + "`", span)
    if index < 0 or index >= len(items):
        return None
    return Some(items.pop(index))


def tpz_array_map(value: object, callback: object, span: tuple[int, int, int]) -> list[object]:
    if not isinstance(value, list):
        tpz_fault("TPZ5001", "`map` takes an array receiver", span)
    if not callable(callback):
        tpz_fault("TPZ5001", "`map` callback must be callable", span)
    return [callback(item) for item in list(value)]


def tpz_array_map__co(value: object, callback: object, span: tuple[int, int, int]) -> object:
    if not isinstance(value, list):
        tpz_fault("TPZ5001", "`map` takes an array receiver", span)
    if not callable(callback):
        tpz_fault("TPZ5001", "`map` callback must be callable", span)
    out: list[object] = []
    for item in list(value):
        out.append((yield from _tpz_call_callback_co(callback, (item,), span)))
    return out


def tpz_array_filter(value: object, callback: object, span: tuple[int, int, int]) -> list[object]:
    if not isinstance(value, list):
        tpz_fault("TPZ5001", "`filter` takes an array receiver", span)
    if not callable(callback):
        tpz_fault("TPZ5001", "`filter` callback must be callable", span)
    out: list[object] = []
    for item in list(value):
        if tpz_condition(callback(item), span):
            out.append(item)
    return out


def tpz_array_filter__co(value: object, callback: object, span: tuple[int, int, int]) -> object:
    if not isinstance(value, list):
        tpz_fault("TPZ5001", "`filter` takes an array receiver", span)
    if not callable(callback):
        tpz_fault("TPZ5001", "`filter` callback must be callable", span)
    out: list[object] = []
    for item in list(value):
        if tpz_condition((yield from _tpz_call_callback_co(callback, (item,), span)), span):
            out.append(item)
    return out


def tpz_array_reduce(value: object, initial: object, callback: object, span: tuple[int, int, int]) -> object:
    if not isinstance(value, list):
        tpz_fault("TPZ5001", "`reduce` takes an array receiver", span)
    if not callable(callback):
        tpz_fault("TPZ5001", "`reduce` callback must be callable", span)
    acc = initial
    for item in list(value):
        acc = callback(acc, item)
    return acc


def tpz_array_reduce__co(value: object, initial: object, callback: object, span: tuple[int, int, int]) -> object:
    if not isinstance(value, list):
        tpz_fault("TPZ5001", "`reduce` takes an array receiver", span)
    if not callable(callback):
        tpz_fault("TPZ5001", "`reduce` callback must be callable", span)
    acc = initial
    for item in list(value):
        acc = yield from _tpz_call_callback_co(callback, (acc, item), span)
    return acc


def _order_key(value: object, method: str, span: tuple[int, int, int]) -> tuple[int, object]:
    if type(value) is int:
        return (0, value)
    if type(value) is float:
        return (1, value)
    if isinstance(value, str):
        return (2, value)
    if isinstance(value, TpzBytes):
        return (3, value.data)
    if _is_topaz_enum(value):
        if value.enum_id == "RoundingMode":
            _cmp_guard_not_comparable("RoundingMode", span)
        return (
            4,
            _tpz_nominal_declaration_identity(value),
            value.variant_index,
            tuple(_order_key(item, method, span) for item in value.payloads),
        )
    tpz_fault("TPZ5001", "`" + method + "` key is not order-comparable: `" + tpz_kind(value) + "`", span)


def tpz_array_sorted(value: object, span: tuple[int, int, int]) -> list[object]:
    if not isinstance(value, list):
        tpz_fault("TPZ5001", "`sorted` takes an array receiver", span)
    keyed = [(_order_key(item, "sorted", span), idx, item) for idx, item in enumerate(list(value))]
    return [item for _key, _idx, item in sorted(keyed, key=lambda entry: (entry[0], entry[1]))]


def tpz_array_sort(value: object, span: tuple[int, int, int]) -> object:
    if not isinstance(value, list):
        tpz_fault("TPZ5001", "`sort` takes an array receiver", span)
    keyed = [(_order_key(item, "sort", span), idx, item) for idx, item in enumerate(list(value))]
    value[:] = [item for _key, _idx, item in sorted(keyed, key=lambda entry: (entry[0], entry[1]))]
    return TPZ_UNIT


def tpz_array_sorted_by(value: object, callback: object, span: tuple[int, int, int]) -> list[object]:
    if not isinstance(value, list):
        tpz_fault("TPZ5001", "`sortedBy` takes an array receiver", span)
    if not callable(callback):
        tpz_fault("TPZ5001", "`sortedBy` callback must be callable", span)
    items = list(value)
    keys = [callback(item) for item in items]
    keyed = [(_order_key(key, "sortedBy", span), idx, item) for idx, (key, item) in enumerate(zip(keys, items))]
    return [item for _key, _idx, item in sorted(keyed, key=lambda entry: (entry[0], entry[1]))]


def tpz_array_sorted_by__co(value: object, callback: object, span: tuple[int, int, int]) -> object:
    if not isinstance(value, list):
        tpz_fault("TPZ5001", "`sortedBy` takes an array receiver", span)
    if not callable(callback):
        tpz_fault("TPZ5001", "`sortedBy` callback must be callable", span)
    items = list(value)
    keys: list[object] = []
    for item in items:
        keys.append((yield from _tpz_call_callback_co(callback, (item,), span)))
    keyed = [(_order_key(key, "sortedBy", span), idx, item) for idx, (key, item) in enumerate(zip(keys, items))]
    return [item for _key, _idx, item in sorted(keyed, key=lambda entry: (entry[0], entry[1]))]


def tpz_array_sort_by(value: object, callback: object, span: tuple[int, int, int]) -> object:
    if not isinstance(value, list):
        tpz_fault("TPZ5001", "`sortBy` takes an array receiver", span)
    if not callable(callback):
        tpz_fault("TPZ5001", "`sortBy` callback must be callable", span)
    items = list(value)
    keys = [callback(item) for item in items]
    keyed = [(_order_key(key, "sortBy", span), idx, item) for idx, (key, item) in enumerate(zip(keys, items))]
    value[:] = [item for _key, _idx, item in sorted(keyed, key=lambda entry: (entry[0], entry[1]))]
    return TPZ_UNIT


def tpz_array_sort_by__co(value: object, callback: object, span: tuple[int, int, int]) -> object:
    if not isinstance(value, list):
        tpz_fault("TPZ5001", "`sortBy` takes an array receiver", span)
    if not callable(callback):
        tpz_fault("TPZ5001", "`sortBy` callback must be callable", span)
    items = list(value)
    keys: list[object] = []
    for item in items:
        keys.append((yield from _tpz_call_callback_co(callback, (item,), span)))
    keyed = [(_order_key(key, "sortBy", span), idx, item) for idx, (key, item) in enumerate(zip(keys, items))]
    value[:] = [item for _key, _idx, item in sorted(keyed, key=lambda entry: (entry[0], entry[1]))]
    return TPZ_UNIT


def tpz_array_retain(value: object, callback: object, span: tuple[int, int, int]) -> object:
    if not isinstance(value, list):
        tpz_fault("TPZ5001", "`retain` takes an array receiver", span)
    if not callable(callback):
        tpz_fault("TPZ5001", "`retain` callback must be callable", span)
    kept: list[object] = []
    for item in list(value):
        if tpz_condition(callback(item), span):
            kept.append(item)
    value[:] = kept
    return TPZ_UNIT


def tpz_array_retain__co(value: object, callback: object, span: tuple[int, int, int]) -> object:
    if not isinstance(value, list):
        tpz_fault("TPZ5001", "`retain` takes an array receiver", span)
    if not callable(callback):
        tpz_fault("TPZ5001", "`retain` callback must be callable", span)
    kept: list[object] = []
    for item in list(value):
        if tpz_condition((yield from _tpz_call_callback_co(callback, (item,), span)), span):
            kept.append(item)
    value[:] = kept
    return TPZ_UNIT


def tpz_option_map(value: object, callback: object, span: tuple[int, int, int]) -> object:
    if value is None:
        return None
    if not isinstance(value, Some):
        tpz_fault("TPZ5001", "`Option.map` takes an Option receiver", span)
    if not callable(callback):
        tpz_fault("TPZ5001", "`Option.map` callback must be callable", span)
    return Some(callback(value.value))


def tpz_option_map__co(value: object, callback: object, span: tuple[int, int, int]) -> object:
    if value is None:
        return None
    if not isinstance(value, Some):
        tpz_fault("TPZ5001", "`Option.map` takes an Option receiver", span)
    if not callable(callback):
        tpz_fault("TPZ5001", "`Option.map` callback must be callable", span)
    return Some((yield from _tpz_call_callback_co(callback, (value.value,), span)))


def tpz_option_flat_map(value: object, callback: object, span: tuple[int, int, int]) -> object:
    if value is None:
        return None
    if not isinstance(value, Some):
        tpz_fault("TPZ5001", "`Option.flatMap` takes an Option receiver", span)
    if not callable(callback):
        tpz_fault("TPZ5001", "`Option.flatMap` callback must be callable", span)
    out = callback(value.value)
    if out is None or isinstance(out, Some):
        return out
    tpz_fault("TPZ5001", "`Option.flatMap` callback must return Option", span)


def tpz_option_flat_map__co(value: object, callback: object, span: tuple[int, int, int]) -> object:
    if value is None:
        return None
    if not isinstance(value, Some):
        tpz_fault("TPZ5001", "`Option.flatMap` takes an Option receiver", span)
    if not callable(callback):
        tpz_fault("TPZ5001", "`Option.flatMap` callback must be callable", span)
    out = yield from _tpz_call_callback_co(callback, (value.value,), span)
    if out is None or isinstance(out, Some):
        return out
    tpz_fault("TPZ5001", "`Option.flatMap` callback must return Option", span)


def tpz_option_ok_or_else(value: object, callback: object, span: tuple[int, int, int]) -> object:
    if isinstance(value, Some):
        return Ok(value.value)
    if value is None:
        if not callable(callback):
            tpz_fault("TPZ5001", "`Option.okOrElse` callback must be callable", span)
        return Err(callback())
    tpz_no_member(value, "okOrElse", span)


def tpz_option_ok_or_else__co(value: object, callback: object, span: tuple[int, int, int]) -> object:
    if isinstance(value, Some):
        return Ok(value.value)
    if value is None:
        if not callable(callback):
            tpz_fault("TPZ5001", "`Option.okOrElse` callback must be callable", span)
        return Err((yield from _tpz_call_callback_co(callback, (), span)))
    tpz_no_member(value, "okOrElse", span)


def tpz_option_ok_or(value: object, error: object, span: tuple[int, int, int]) -> object:
    if isinstance(value, Some):
        return Ok(value.value)
    if value is None:
        return Err(error)
    tpz_no_member(value, "okOr", span)


def tpz_result_map(value: object, callback: object, span: tuple[int, int, int]) -> object:
    if isinstance(value, Err):
        return value
    if not isinstance(value, Ok):
        tpz_fault("TPZ5001", "`Result.map` takes a Result receiver", span)
    if not callable(callback):
        tpz_fault("TPZ5001", "`Result.map` callback must be callable", span)
    return Ok(callback(value.value))


def tpz_result_map__co(value: object, callback: object, span: tuple[int, int, int]) -> object:
    if isinstance(value, Err):
        return value
    if not isinstance(value, Ok):
        tpz_fault("TPZ5001", "`Result.map` takes a Result receiver", span)
    if not callable(callback):
        tpz_fault("TPZ5001", "`Result.map` callback must be callable", span)
    return Ok((yield from _tpz_call_callback_co(callback, (value.value,), span)))


def tpz_result_flat_map(value: object, callback: object, span: tuple[int, int, int]) -> object:
    if isinstance(value, Err):
        return value
    if not isinstance(value, Ok):
        tpz_fault("TPZ5001", "`Result.flatMap` takes a Result receiver", span)
    if not callable(callback):
        tpz_fault("TPZ5001", "`Result.flatMap` callback must be callable", span)
    out = callback(value.value)
    if isinstance(out, Ok) or isinstance(out, Err):
        return out
    tpz_fault("TPZ5001", "`Result.flatMap` callback must return Result", span)


def tpz_result_flat_map__co(value: object, callback: object, span: tuple[int, int, int]) -> object:
    if isinstance(value, Err):
        return value
    if not isinstance(value, Ok):
        tpz_fault("TPZ5001", "`Result.flatMap` takes a Result receiver", span)
    if not callable(callback):
        tpz_fault("TPZ5001", "`Result.flatMap` callback must be callable", span)
    out = yield from _tpz_call_callback_co(callback, (value.value,), span)
    if isinstance(out, Ok) or isinstance(out, Err):
        return out
    tpz_fault("TPZ5001", "`Result.flatMap` callback must return Result", span)


def tpz_index(value: object, index: object, span: tuple[int, int, int]) -> object:
    if isinstance(value, str):
        tpz_fault("TPZ5001", "strings are not indexable; use `s.scalars()` (§1)", span)
    if isinstance(value, list):
        if type(index) is not int:
            tpz_fault("TPZ5001", "cannot index `Array` with `" + tpz_kind(index) + "`", span)
        if index < 0 or index >= len(value):
            tpz_fault(
                "TPZ4001",
                "index " + str(index) + " is out of bounds for an array of length " + str(len(value)),
                span,
            )
        return value[index]
    tpz_fault(
        "TPZ5001",
        "cannot index `" + tpz_kind(value) + "` with `" + tpz_kind(index) + "`",
        span,
    )


def tpz_index_slot(value: object, index: object, span: tuple[int, int, int]) -> tuple[list, int]:
    if not isinstance(value, list):
        tpz_fault(
            "TPZ5001",
            "cannot index-assign `" + tpz_kind(value) + "`; only Array cells are index-assignable (§9)",
            span,
        )
    if type(index) is not int:
        tpz_fault("TPZ5001", "array indices are `int`, found `" + tpz_kind(index) + "`", span)
    if index < 0 or index >= len(value):
        tpz_fault("TPZ4001", "index " + str(index) + " out of bounds for length " + str(len(value)) + " (§13a)", span)
    return (value, index)


def tpz_index_slot_set(slot: tuple[list, int], value: object) -> None:
    items, index = slot
    items[index] = value


def tpz_index_slot_get(slot: tuple[list, int]) -> object:
    items, index = slot
    return items[index]


def tpz_index_slot_is_empty(slot: tuple[list, int]) -> bool:
    items, index = slot
    return items[index] is None or items[index] is TPZ_NULL


def tpz_immutable_assignment(name: object, span: tuple[int, int, int]) -> None:
    if not isinstance(name, str):
        tpz_fault("TPZ5001", "immutable assignment metadata is malformed", span)
    tpz_fault("TPZ5003", "`" + name + "` is not `let mut` and cannot be assigned", span)


def tpz_render(value: object) -> str:
    if type(value) is bool:
        return "true" if value else "false"
    if type(value) is float:
        return tpz_format_f64(value)
    if value is TPZ_UNIT:
        return "()"
    if value is TPZ_NULL:
        return "null"
    if value is None:
        return "None"
    if isinstance(value, Some):
        return "Some(" + tpz_render(value.value) + ")"
    if isinstance(value, Ok):
        return "Ok(" + tpz_render(value.value) + ")"
    if isinstance(value, Err):
        return "Err(" + tpz_render(value.value) + ")"
    if isinstance(value, TpzBytes):
        return "Bytes(" + tpz_bytes_to_hex(value) + ")"
    if isinstance(value, TpzByteBuffer):
        return "ByteBuffer(length: " + str(len(value.data)) + ")"
    if isinstance(value, TpzRegex):
        return "Regex(" + value.pattern + ")"
    if isinstance(value, TpzUrl):
        return value.canonical
    if isinstance(value, TpzMap):
        parts = []
        for key, item in value.entries:
            parts.append(tpz_render(_key_to_value(key)) + ": " + tpz_render(item))
        return "Map{" + ", ".join(parts) + "}"
    if isinstance(value, TpzSet):
        return "Set{" + ", ".join(tpz_render(_key_to_value(key)) for key in value.items) + "}"
    if isinstance(value, TpzRange):
        out = str(value.lo) + (".." if value.inclusive else "..<") + str(value.hi)
        if value.step != 1:
            out += " by " + str(value.step)
        return out
    if isinstance(value, TpzTemplate):
        if value.tag == "p":
            return value.normalized
        return (
            "<"
            + value.tag
            + " template, "
            + str(len(value.parts))
            + " part(s), "
            + str(len(value.values))
            + " interpolation(s)>"
        )
    if isinstance(value, list):
        return "[" + ", ".join(tpz_render(item) for item in value) + "]"
    if _is_topaz_newtype(value):
        return value.newtype_id + "(" + tpz_render(value.value) + ")"
    if _is_topaz_enum(value):
        if len(value.payloads) == 0:
            return value.enum_id + "." + value.variant
        return value.enum_id + "." + value.variant + "(" + ", ".join(tpz_render(item) for item in value.payloads) + ")"
    if _is_topaz_nominal_record(value):
        fields = [
            source_field + ": " + tpz_render(getattr(value, py_field))
            for py_field, source_field in value.__topaz_record_fields__
        ]
        if not fields:
            return value.__topaz_record_id__ + " {}"
        return value.__topaz_record_id__ + " { " + ", ".join(fields) + " }"
    if _is_topaz_record(value):
        fields = [
            source_field + ": " + tpz_render(getattr(value, py_field))
            for py_field, source_field in sorted(
                value.__topaz_record_fields__, key=lambda item: item[1]
            )
        ]
        return "{ " + ", ".join(fields) + " }"
    return str(value)
