"""Mechanical Python target adapter for validated Topaz fixed-point IR.

This module is compiler output support, not a Python front end.  It never
reads Topaz source and refuses every operation or runtime leaf it has not been
explicitly taught to consume.
"""

from __future__ import annotations

import json

from topaz_py_rt import (
    DeploymentHost,
    Err,
    Host,
    Ok,
    Some,
    TPZ_UNIT,
    TpzFault,
    TpzLoopBreak,
    TpzLoopContinue,
    TpzReturn,
    tpz_add,
    tpz_byte_buffer_allocate,
    tpz_byte_buffer_copy,
    tpz_byte_buffer_fill,
    tpz_byte_buffer_from_bytes,
    tpz_byte_buffer_get,
    tpz_byte_buffer_length,
    tpz_byte_buffer_set,
    tpz_byte_buffer_to_bytes,
    tpz_bytes_to_hex,
    tpz_div,
    tpz_eq,
    tpz_for_items,
    tpz_ge,
    tpz_gt,
    tpz_le,
    tpz_lt,
    tpz_mul,
    tpz_member,
    tpz_ne,
    tpz_neg,
    tpz_render,
    tpz_sub,
    tpz_to_int,
    tpz_try,
)

IR_SCHEMA = "topaz.compiler.fixed-point-ir-payload/v1"
FACTS_SCHEMA = "topaz.self-target-adapter-facts/v1"
STEP_LIMIT = 10_000_000


class _Function:
    def __init__(self, operation: int) -> None:
        self.operation = operation


class _Type:
    def __init__(self, name: str) -> None:
        self.name = name


class _BoundMethod:
    def __init__(self, receiver: object, name: str) -> None:
        self.receiver = receiver
        self.name = name


class _NominalRecord:
    def __init__(self, identity: str, fields: list[tuple[str, object]]) -> None:
        self.__topaz_record_id__ = identity
        self.__topaz_record_fields__ = tuple((name, name) for name, _ in fields)
        self._fields = dict(fields)

    def __getattr__(self, name: str) -> object:
        try:
            return self._fields[name]
        except KeyError as error:
            raise AttributeError(name) from error


class _Environment:
    def __init__(self, parent: "_Environment | None" = None) -> None:
        self.parent = parent
        self.values: dict[str, object] = {}

    def get(self, key: str) -> object:
        current: _Environment | None = self
        while current is not None:
            if key in current.values:
                return current.values[key]
            current = current.parent
        raise KeyError(key)

    def set_existing(self, key: str, value: object) -> bool:
        current: _Environment | None = self
        while current is not None:
            if key in current.values:
                current.values[key] = value
                return True
            current = current.parent
        return False


class _Machine:
    def __init__(self, ir_json: str, facts_json: str, host: Host, args: list[str]) -> None:
        payload = json.loads(ir_json)
        facts = json.loads(facts_json)
        if payload.get("schema") != IR_SCHEMA:
            raise ValueError("self Python target fixed-point IR schema mismatch")
        if facts.get("schema") != FACTS_SCHEMA:
            raise ValueError("self Python target facts schema mismatch")
        self.operations = payload.get("loweredOperations")
        self.modules = payload.get("loweredModules")
        if not isinstance(self.operations, list) or not isinstance(self.modules, list):
            raise ValueError("self Python target payload omitted operation tables")
        self.index = {
            operation["id"]: index for index, operation in enumerate(self.operations)
        }
        if len(self.index) != len(self.operations):
            raise ValueError("self Python target payload duplicates an operation id")
        self.host = host
        self.args = args
        self.globals = _Environment()
        self.functions: dict[str, int] = {}
        self.nominals: dict[str, dict[str, object]] = {}
        self.operation_nominals: dict[str, dict[str, object]] = {}
        self.steps = 0
        self._register_nominals(facts)

    def _register_nominals(self, facts: dict[str, object]) -> None:
        raw_nominals = facts.get("nominals")
        raw_operation_nominals = facts.get("operationNominals")
        if not isinstance(raw_nominals, list) or not isinstance(
            raw_operation_nominals, list
        ):
            raise ValueError("self Python target facts omitted nominal tables")
        for nominal in raw_nominals:
            if not isinstance(nominal, dict):
                raise ValueError("self Python target nominal fact is malformed")
            name = nominal.get("name")
            identity = nominal.get("identity")
            if not isinstance(name, str) or not isinstance(identity, str):
                raise ValueError("self Python target nominal identity is malformed")
            self.nominals[name] = nominal
            self.nominals[identity] = nominal
        for row in raw_operation_nominals:
            if not isinstance(row, dict) or not isinstance(row.get("operationId"), str):
                raise ValueError("self Python target operation nominal fact is malformed")
            identity = row.get("identity")
            if not isinstance(identity, str) or identity not in self.nominals:
                raise ValueError("self Python target operation nominal identity is missing")
            self.operation_nominals[row["operationId"]] = self.nominals[identity]

    def operation(self, identity: str) -> int:
        try:
            return self.index[identity]
        except KeyError as error:
            raise ValueError(
                "self Python target operation refers to missing id `" + identity + "`"
            ) from error

    def register_functions(self) -> None:
        for index, operation in enumerate(self.operations):
            if operation.get("kind") == "function" and operation.get("bindingName"):
                identity = operation["module"] + "::" + operation["bindingName"]
                self.functions[identity] = index

    def initialize(self) -> None:
        for module in self.modules:
            for identity in module.get("operationIds", []):
                operation = self.operations[self.operation(identity)]
                if operation.get("kind") in {"module", "export", "constant"}:
                    self.eval(self.operation(identity), self.globals)

    def run(self) -> tuple[bool, object]:
        self.register_functions()
        self.initialize()
        entries = [module for module in self.modules if module.get("entry")]
        if len(entries) != 1:
            raise ValueError(
                "self Python target requires exactly one entry module, found "
                + str(len(entries))
            )
        main = self.functions.get(entries[0]["identity"] + "::main")
        if main is None:
            return (False, TPZ_UNIT)
        parameters = [
            self.operations[self.operation(identity)]
            for identity in self.operations[main].get("operands", [])
            if self.operations[self.operation(identity)].get("kind")
            == "binding/parameter"
        ]
        if len(parameters) == 0:
            return (True, self.call_function(main, []))
        if len(parameters) == 1:
            return (True, self.call_function(main, [list(self.args)]))
        if len(parameters) == 2:
            return (True, self.call_function(main, [list(self.args), self.host.input()]))
        raise ValueError("self Python target main supports zero, one, or two parameters")

    def call_function(self, function: int, arguments: list[object]) -> object:
        operation = self.operations[function]
        environment = _Environment(self.globals)
        parameters: list[int] = []
        body: int | None = None
        for identity in operation.get("operands", []):
            child = self.operation(identity)
            kind = self.operations[child].get("kind")
            if kind == "binding/parameter":
                parameters.append(child)
            elif kind == "expression/block":
                body = child
        required = next(
            (
                index
                for index, parameter in enumerate(parameters)
                if self.operations[parameter].get("operands")
            ),
            len(parameters),
        )
        if len(arguments) < required or len(arguments) > len(parameters):
            raise ValueError(
                operation["module"]
                + "::"
                + operation.get("bindingName", "")
                + " expects "
                + str(required)
                + ".."
                + str(len(parameters))
                + " arguments, found "
                + str(len(arguments))
            )
        for index, parameter in enumerate(parameters):
            if index < len(arguments):
                argument = arguments[index]
            else:
                defaults = self.operations[parameter].get("operands", [])
                if not defaults:
                    raise ValueError("self Python target omitted a required argument")
                argument = self.eval(self.operation(defaults[0]), environment)
            self.bind(parameter, argument, environment)
        if body is None:
            raise ValueError("self Python target function omitted its body")
        try:
            return self.eval(body, environment)
        except TpzReturn as returned:
            return returned.value

    def bind(self, pattern: int, value: object, environment: _Environment) -> None:
        operation = self.operations[pattern]
        if operation.get("kind") not in {
            "pattern/binding",
            "pattern/typed-binding",
            "binding/parameter",
        }:
            raise ValueError(
                "unsupported self Python binding operation `"
                + str(operation.get("kind"))
                + "`"
            )
        for key in (
            operation.get("bindingName", ""),
            operation.get("declarationIdentity", ""),
        ):
            if key:
                environment.values[key] = value

    def eval(self, index: int, environment: _Environment) -> object:
        self.steps += 1
        if self.steps > STEP_LIMIT:
            raise ValueError("self Python target execution-step limit exceeded")
        operation = self.operations[index]
        kind = operation.get("kind")
        operands = [self.operation(identity) for identity in operation.get("operands", [])]
        if kind in {"module", "export", "expression/block"}:
            value: object = TPZ_UNIT
            for operand in operands:
                value = self.eval(operand, environment)
            return value
        if kind in {
            "import",
            "record",
            "enum",
            "newtype",
            "type-alias",
            "function",
            "binding/capture",
        }:
            return TPZ_UNIT
        if kind == "constant":
            value = self.eval(operands[0], environment)
            self._define(operation, value, self.globals)
            return value
        if kind == "expression/integer":
            return int(operation.get("detail", "0"))
        if kind == "expression/boolean":
            return operation.get("detail") == "true"
        if kind == "expression/unit":
            return TPZ_UNIT
        if kind == "expression/string-text":
            return operation.get("detail", "")
        if kind == "expression/string":
            parts = [self.eval(operand, environment) for operand in operands]
            return "".join(part if isinstance(part, str) else tpz_render(part) for part in parts)
        if kind == "expression/identifier":
            for key in (
                operation.get("referenceIdentity", ""),
                operation.get("detail", ""),
            ):
                if not key:
                    continue
                try:
                    return environment.get(key)
                except KeyError:
                    pass
                if key in self.functions:
                    return _Function(self.functions[key])
            return _Type(operation.get("detail", ""))
        if kind == "expression/array":
            return [self.eval(operand, environment) for operand in operands]
        if kind == "expression/member":
            if len(operands) != 1:
                raise ValueError("self Python target member has no receiver")
            receiver = self.eval(operands[0], environment)
            if isinstance(receiver, _Type):
                return _BoundMethod(receiver, operation.get("detail", ""))
            return tpz_member(
                receiver,
                operation.get("detail", ""),
                operation.get("detail", ""),
                self.span(operation),
            )
        if kind == "expression/call":
            return self.eval_call(operation, operands, environment)
        if kind in {"expression/record-literal", "expression/record-update"}:
            return self.eval_record(operation, operands, environment)
        if kind == "expression/binary":
            left = self.eval(operands[0], environment)
            detail = operation.get("detail", "")
            if detail in {"and", "or"}:
                if not isinstance(left, bool):
                    raise ValueError("self Python boolean operator requires bool")
                if detail == "and" and not left:
                    return False
                if detail == "or" and left:
                    return True
            right = self.eval(operands[1], environment)
            return self.binary(detail, left, right, self.span(operation))
        if kind == "expression/unary":
            value = self.eval(operands[0], environment)
            if operation.get("detail") in {"neg", "minus"}:
                return tpz_neg(value, self.span(operation))
            if operation.get("detail") in {"pos", "plus"}:
                return value
            if operation.get("detail") == "not":
                return not value
            raise ValueError("unsupported self Python unary operator")
        if kind == "let":
            value = self.eval(operands[0], environment)
            self.bind(operands[1], value, environment)
            return TPZ_UNIT
        if kind == "assignment":
            value = self.eval(operands[-1], environment)
            target = self.operations[operands[0]]
            keys = (
                target.get("referenceIdentity", ""),
                target.get("detail", ""),
            )
            if not any(key and environment.set_existing(key, value) for key in keys):
                raise ValueError("self Python assignment target is missing")
            return value
        if kind == "expression/if":
            condition = self.eval(operands[0], environment)
            if type(condition) is not bool:
                raise ValueError("self Python if condition is not bool")
            if condition:
                return self.eval(operands[1], environment)
            if len(operands) == 3:
                return self.eval(operands[2], environment)
            return TPZ_UNIT
        if kind == "expression/match":
            return self.eval_match(operation, operands, environment)
        if kind == "expression/for":
            items = tpz_for_items(self.eval(operands[0], environment), self.span(operation))
            for item in items:
                loop_environment = _Environment(environment)
                if not self.match_pattern(operands[1], item, loop_environment):
                    raise ValueError("self Python iterator pattern did not match")
                try:
                    self.eval(operands[2], loop_environment)
                except TpzLoopContinue:
                    continue
                except TpzLoopBreak:
                    break
            return TPZ_UNIT
        if kind == "expression/result-propagation":
            return tpz_try(self.eval(operands[0], environment), self.span(operation))
        if kind == "return":
            value = self.eval(operands[0], environment) if operands else TPZ_UNIT
            raise TpzReturn(value)
        if kind == "break":
            raise TpzLoopBreak(None, TPZ_UNIT)
        if kind == "continue":
            raise TpzLoopContinue(None)
        raise ValueError("unsupported self Python target operation `" + str(kind) + "`")

    def eval_record(
        self, operation: dict[str, object], operands: list[int], environment: _Environment
    ) -> object:
        fields: dict[str, object] = {}
        nominal: dict[str, object] | None = None
        labels = operation.get("operandLabels", [])
        if not isinstance(labels, list) or len(labels) != len(operands):
            raise ValueError("self Python record operand labels are malformed")
        for operand, raw_label in zip(operands, labels):
            if not isinstance(raw_label, str):
                raise ValueError("self Python record field label is malformed")
            value = self.eval(operand, environment)
            if raw_label.startswith("base:"):
                if isinstance(value, _Type):
                    nominal = self.nominals.get(value.name)
                    if nominal is None or nominal.get("kind") != "record":
                        raise ValueError("self Python record base is not a known record type")
                elif isinstance(value, _NominalRecord):
                    nominal = self.nominals.get(value.__topaz_record_id__)
                    fields.update(value._fields)
                else:
                    raise ValueError("self Python record base is not a record")
                continue
            marker = raw_label.find("field-initializer[")
            equals = raw_label.find("=", marker)
            if marker < 0 or equals < 0:
                raise ValueError("self Python record field label is unsupported")
            name = raw_label[equals + 1 :].split("/", 1)[0]
            fields[name] = value
        nominal = nominal or self.operation_nominals.get(str(operation.get("id", "")))
        if nominal is None:
            raise ValueError("self Python record literal has no nominal fact")
        identity = nominal.get("identity")
        members = nominal.get("members")
        if not isinstance(identity, str) or not isinstance(members, list):
            raise ValueError("self Python record nominal fact is malformed")
        ordered: list[tuple[str, object]] = []
        for member in members:
            if not isinstance(member, dict) or not isinstance(member.get("name"), str):
                raise ValueError("self Python record member fact is malformed")
            name = member["name"]
            if name in fields:
                value = fields.pop(name)
            else:
                default_identity = member.get("defaultOperationId")
                if not isinstance(default_identity, str):
                    raise ValueError(
                        "self Python record `" + identity + "` is missing field `" + name + "`"
                    )
                value = self.eval(self.operation(default_identity), environment)
            ordered.append((name, value))
        if fields:
            raise ValueError("self Python record has unprojected fields")
        return _NominalRecord(identity, ordered)

    def eval_match(
        self, operation: dict[str, object], operands: list[int], environment: _Environment
    ) -> object:
        scrutinee = self.eval(operands[0], environment)
        labels = operation.get("operandLabels", [])
        index = 1
        while index < len(operands):
            label = labels[index]
            if not isinstance(label, str) or "/pattern:" not in label:
                raise ValueError("self Python match pattern label is malformed")
            case_environment = _Environment(environment)
            matched = self.match_pattern(operands[index], scrutinee, case_environment)
            index += 1
            if index < len(operands) and "/guard:" in labels[index]:
                guarded = self.eval(operands[index], case_environment) if matched else False
                if type(guarded) is not bool:
                    raise ValueError("self Python match guard is not bool")
                matched = guarded
                index += 1
            if index >= len(operands) or "/body:" not in labels[index]:
                raise ValueError("self Python match body label is malformed")
            body = operands[index]
            body_label = labels[index]
            index += 1
            if matched:
                value = self.eval(body, case_environment)
                if "match-case-return" in body_label:
                    raise TpzReturn(value)
                return value
        raise ValueError("self Python match has no matching case")

    def match_pattern(
        self, pattern: int, value: object, environment: _Environment
    ) -> bool:
        operation = self.operations[pattern]
        kind = operation.get("kind")
        operands = [self.operation(identity) for identity in operation.get("operands", [])]
        if kind == "pattern/wildcard":
            return True
        if kind in {"pattern/binding", "pattern/typed-binding", "binding/parameter"}:
            self.bind(pattern, value, environment)
            return True
        if kind == "pattern/literal":
            return tpz_eq(
                self.eval(operands[0], environment), value, self.span(operation)
            )
        if kind == "pattern/constructor":
            detail = operation.get("detail")
            payloads: list[object]
            if detail == "None" and value is None:
                payloads = []
            elif detail == "Some" and isinstance(value, Some):
                payloads = [value.value]
            elif detail == "Ok" and isinstance(value, Ok):
                payloads = [value.value]
            elif detail == "Err" and isinstance(value, Err):
                payloads = [value.value]
            else:
                return False
            return len(payloads) == len(operands) and all(
                self.match_pattern(child, payload, environment)
                for child, payload in zip(operands, payloads)
            )
        raise ValueError("unsupported self Python pattern `" + str(kind) + "`")

    def eval_call(
        self, operation: dict[str, object], operands: list[int], environment: _Environment
    ) -> object:
        target = str(operation.get("callTarget", ""))
        span = self.span(operation)
        callee_operation = self.operations[operands[0]]
        if callee_operation.get("kind") == "expression/member":
            receiver_operands = callee_operation.get("operands", [])
            if len(receiver_operands) != 1:
                raise ValueError("self Python method callee has no receiver")
            callee = _BoundMethod(
                self.eval(self.operation(receiver_operands[0]), environment),
                str(callee_operation.get("detail", "")),
            )
        else:
            callee = self.eval(operands[0], environment)
        arguments = [self.eval(operand, environment) for operand in operands[1:]]
        if target == "builtin::print":
            if len(arguments) != 1:
                raise ValueError("self Python print expects one argument")
            return self.host.print(arguments[0], span)
        if target == "builtin::toInt":
            if len(arguments) != 1:
                raise ValueError("self Python toInt expects one argument")
            return tpz_to_int(arguments[0], span)
        if target == "builtin::Some":
            return Some(self._one(arguments, "Some"))
        if target == "builtin::None":
            if arguments:
                raise ValueError("self Python None expects no arguments")
            return None
        if target == "builtin::Ok":
            return Ok(self._one(arguments, "Ok"))
        if target == "builtin::Err":
            return Err(self._one(arguments, "Err"))
        if isinstance(callee, _Function):
            return self.call_function(callee.operation, arguments)
        if isinstance(callee, _BoundMethod):
            if isinstance(callee.receiver, _Type):
                return self.call_static(callee.receiver.name, callee.name, arguments, span)
            return self.call_method(callee.receiver, callee.name, arguments, span)
        if isinstance(callee, _Type):
            return self.call_static(callee.name, str(operation.get("callMethod", "")), arguments, span)
        raise ValueError("self Python call target is not callable")

    @staticmethod
    def _one(arguments: list[object], name: str) -> object:
        if len(arguments) != 1:
            raise ValueError("self Python " + name + " expects one argument")
        return arguments[0]

    def call_static(
        self, receiver: str, method: str, arguments: list[object], span: tuple[int, int, int]
    ) -> object:
        if receiver == "ByteBuffer" and method == "allocate":
            return tpz_byte_buffer_allocate(*arguments, span=span)
        if receiver == "ByteBuffer" and method == "fromBytes":
            return tpz_byte_buffer_from_bytes(arguments[0], span)
        raise ValueError("unsupported self Python static call `" + receiver + "." + method + "`")

    def call_method(
        self, receiver: object, method: str, arguments: list[object], span: tuple[int, int, int]
    ) -> object:
        if method == "get":
            return tpz_byte_buffer_get(receiver, arguments[0], span)
        if method == "set":
            return tpz_byte_buffer_set(receiver, arguments[0], arguments[1], span)
        if method == "fill":
            return tpz_byte_buffer_fill(receiver, arguments[0], arguments[1], arguments[2], span)
        if method == "copy":
            return tpz_byte_buffer_copy(
                receiver, arguments[0], arguments[1], arguments[2], arguments[3], span
            )
        if method == "toBytes":
            return tpz_byte_buffer_to_bytes(receiver, span)
        if method == "toHex":
            return tpz_bytes_to_hex(receiver, span)
        if method == "length":
            return tpz_byte_buffer_length(receiver, span)
        raise ValueError("unsupported self Python method call `" + method + "`")

    @staticmethod
    def binary(
        operator: str, left: object, right: object, span: tuple[int, int, int]
    ) -> object:
        functions = {
            "add": tpz_add,
            "plus": tpz_add,
            "sub": tpz_sub,
            "minus": tpz_sub,
            "subtract": tpz_sub,
            "mul": tpz_mul,
            "times": tpz_mul,
            "multiply": tpz_mul,
            "div": tpz_div,
            "divide": tpz_div,
            "eq": tpz_eq,
            "equal": tpz_eq,
            "ne": tpz_ne,
            "not-equal": tpz_ne,
            "lt": tpz_lt,
            "less-than": tpz_lt,
            "le": tpz_le,
            "less-or-equal": tpz_le,
            "gt": tpz_gt,
            "greater-than": tpz_gt,
            "ge": tpz_ge,
            "greater-or-equal": tpz_ge,
        }
        try:
            return functions[operator](left, right, span)
        except KeyError as error:
            raise ValueError(
                "unsupported self Python binary operator `" + operator + "`"
            ) from error

    @staticmethod
    def span(operation: dict[str, object]) -> tuple[int, int, int]:
        return (0, int(operation.get("lo", 0)), int(operation.get("hi", 0)))

    @staticmethod
    def _define(operation: dict[str, object], value: object, environment: _Environment) -> None:
        for key in (
            operation.get("bindingName", ""),
            operation.get("declarationIdentity", ""),
        ):
            if isinstance(key, str) and key:
                environment.values[key] = value


def run_product(
    ir_json: str,
    facts_json: str,
    stdin_text: str,
    args: list[str] | None = None,
) -> int:
    host = DeploymentHost(stdin_text, ".", [], [])
    try:
        machine = _Machine(ir_json, facts_json, host, list(args or []))
        explicit_main, result = machine.run()
        return host.application_exit(result) if explicit_main else 0
    except TpzFault as fault:
        return host.application_fault(fault)
