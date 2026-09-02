use super::{environment::*, machine::*, model::*};
use crate::diagnostic::*;
use crate::program::{decode_json::*, model::*};
use crate::wire::*;
use crate::*;

#[derive(Clone, Debug)]
pub(crate) struct NominalMemberFact {
    pub(crate) name: String,
    pub(crate) arity: usize,
    pub(crate) default_operation_id: Option<String>,
    pub(crate) types: Vec<SemanticType>,
}

#[derive(Clone, Debug)]
pub(crate) struct NominalFact {
    pub(crate) identity: String,
    pub(crate) kind: String,
    pub(crate) type_parameters: Vec<String>,
    pub(crate) members: Vec<NominalMemberFact>,
    pub(crate) base_type: Option<SemanticType>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct NominalRegistry {
    pub(crate) by_identity: BTreeMap<String, NominalFact>,
    pub(crate) by_name: BTreeMap<String, String>,
    pub(crate) by_operation: BTreeMap<String, String>,
}

impl NominalRegistry {
    pub(crate) fn parse(payload: &str) -> Result<Self, String> {
        let parsed = topaz_value::value::json_parse(payload)
            .map_err(|error| format!("self target facts JSON is invalid: {error:?}"))?;
        let root = object(&parsed, "self target facts")?;
        if string(root, "schema", "self target facts")? != TARGET_ADAPTER_FACTS_SCHEMA {
            return Err("self target facts schema mismatch".to_string());
        }
        let rows = array(
            field(root, "nominals", "self target facts")?,
            "self target nominal facts",
        )?;
        let mut registry = Self::default();
        let mut short_names = BTreeMap::<String, Option<String>>::new();
        for (index, row) in rows.iter().enumerate() {
            let context = format!("self target nominal fact {index}");
            let row = object(row, &context)?;
            let name = string(row, "name", &context)?;
            let identity = string(row, "identity", &context)?;
            let kind = string(row, "kind", &context)?;
            if !matches!(kind.as_str(), "record" | "enum" | "newtype") {
                return Err(format!("{context}.kind `{kind}` is unsupported"));
            }
            let type_parameters = match row.get("typeParameters") {
                None => Vec::new(),
                Some(value) => array(value, &format!("{context}.typeParameters"))?
                    .iter()
                    .enumerate()
                    .map(|(index, value)| match value {
                        JsonValue::String(value) => Ok(value.to_string()),
                        _ => Err(format!("{context}.typeParameters[{index}] is not a string")),
                    })
                    .collect::<Result<Vec<_>, String>>()?,
            };
            let mut members = Vec::new();
            for (member_index, member) in array(
                field(row, "members", &context)?,
                &format!("{context}.members"),
            )?
            .iter()
            .enumerate()
            {
                let member_context = format!("{context}.members[{member_index}]");
                let member = object(member, &member_context)?;
                let arity = usize::try_from(integer(member, "arity", &member_context)?)
                    .map_err(|_| format!("{member_context}.arity is too large"))?;
                let types = match member.get("types") {
                    None => Vec::new(),
                    Some(value) => array(value, &format!("{member_context}.types"))?
                        .iter()
                        .enumerate()
                        .map(|(type_index, value)| {
                            let type_context = format!("{member_context}.types[{type_index}]");
                            let JsonValue::String(encoded) = value else {
                                return Err(format!("{type_context} is not a string"));
                            };
                            parse_runtime_type(encoded, &type_context)
                        })
                        .collect::<Result<Vec<_>, String>>()?,
                };
                if !types.is_empty() && types.len() != arity {
                    return Err(format!(
                        "{member_context} has {} types for arity {arity}",
                        types.len()
                    ));
                }
                members.push(NominalMemberFact {
                    name: string(member, "name", &member_context)?,
                    arity,
                    default_operation_id: match member.get("defaultOperationId") {
                        None | Some(JsonValue::Null) => None,
                        Some(JsonValue::String(value)) => Some(value.to_string()),
                        Some(_) => {
                            return Err(format!(
                                "{member_context}.defaultOperationId is not a string or null"
                            ));
                        }
                    },
                    types,
                });
            }
            let base_type = match row.get("baseType") {
                None | Some(JsonValue::Null) => None,
                Some(JsonValue::String(encoded)) => {
                    let type_context = format!("{context}.baseType");
                    Some(parse_runtime_type(encoded, &type_context)?)
                }
                Some(_) => return Err(format!("{context}.baseType is not a string or null")),
            };
            let fact = NominalFact {
                identity: identity.clone(),
                kind,
                type_parameters,
                members,
                base_type,
            };
            if registry
                .by_identity
                .insert(identity.clone(), fact)
                .is_some()
            {
                return Err(format!(
                    "self target facts duplicate nominal identity `{identity}`"
                ));
            }
            short_names
                .entry(name)
                .and_modify(|candidate| *candidate = None)
                .or_insert_with(|| Some(identity));
        }
        registry.by_name = short_names
            .into_iter()
            .filter_map(|(name, identity)| identity.map(|identity| (name, identity)))
            .collect();
        for (index, row) in array(
            field(root, "operationNominals", "self target facts")?,
            "self target operation nominal facts",
        )?
        .iter()
        .enumerate()
        {
            let context = format!("self target operation nominal fact {index}");
            let row = object(row, &context)?;
            let operation_id = string(row, "operationId", &context)?;
            let identity = string(row, "identity", &context)?;
            let kind = string(row, "kind", &context)?;
            let nominal = registry.by_identity.get(&identity).ok_or_else(|| {
                format!("{context} refers to unknown nominal identity `{identity}`")
            })?;
            if nominal.kind != kind {
                return Err(format!(
                    "{context} kind `{kind}` disagrees with nominal kind `{}`",
                    nominal.kind
                ));
            }
            if registry
                .by_operation
                .insert(operation_id.clone(), identity)
                .is_some()
            {
                return Err(format!(
                    "self target facts duplicate operation nominal `{operation_id}`"
                ));
            }
        }
        Ok(registry)
    }

    pub(crate) fn get(&self, name: &str) -> Option<&NominalFact> {
        self.by_identity.get(name).or_else(|| {
            self.by_name
                .get(name)
                .and_then(|identity| self.by_identity.get(identity))
        })
    }

    pub(crate) fn operation(&self, operation_id: &str) -> Option<&NominalFact> {
        self.by_operation
            .get(operation_id)
            .and_then(|identity| self.by_identity.get(identity))
    }

    /// The shared host boundary uses source-level nominal names, while a self
    /// target executes with defining-module identities from its checked facts.
    /// Project only immutable nominal wrappers; opaque and mutable payloads keep
    /// their host-owned identity.
    pub(crate) fn project_host_value(&self, value: Value) -> Value {
        match value {
            Value::Some(value) => {
                Value::Some(Rc::new(self.project_host_value(Rc::unwrap_or_clone(value))))
            }
            Value::Ok(value) => {
                Value::Ok(Rc::new(self.project_host_value(Rc::unwrap_or_clone(value))))
            }
            Value::Err(value) => {
                Value::Err(Rc::new(self.project_host_value(Rc::unwrap_or_clone(value))))
            }
            Value::Enum {
                enum_id,
                declaration_identity,
                method_identity,
                variant,
                variant_index,
                payloads,
            } => {
                let identity =
                    nominal_declaration_identity(&enum_id, declaration_identity.as_deref());
                let projected = self
                    .get(identity)
                    .filter(|fact| fact.kind == "enum")
                    .map(|fact| Rc::from(fact.identity.as_str()));
                let (enum_id, declaration_identity) = match projected {
                    Some(identity) => (identity, None),
                    None => (enum_id, declaration_identity),
                };
                Value::Enum {
                    enum_id,
                    declaration_identity,
                    method_identity,
                    variant,
                    variant_index,
                    payloads: payloads
                        .iter()
                        .cloned()
                        .map(|value| self.project_host_value(value))
                        .collect(),
                }
            }
            Value::NominalRecord {
                record_id,
                declaration_identity,
                method_identity,
                fields,
            } => {
                let identity =
                    nominal_declaration_identity(&record_id, declaration_identity.as_deref());
                let projected = self
                    .get(identity)
                    .filter(|fact| fact.kind == "record")
                    .map(|fact| Rc::from(fact.identity.as_str()));
                let (record_id, declaration_identity) = match projected {
                    Some(identity) => (identity, None),
                    None => (record_id, declaration_identity),
                };
                Value::NominalRecord {
                    record_id,
                    declaration_identity,
                    method_identity,
                    fields: fields
                        .iter()
                        .map(|(name, value)| (name.clone(), self.project_host_value(value.clone())))
                        .collect(),
                }
            }
            Value::Newtype {
                newtype_id,
                declaration_identity,
                method_identity,
                inner,
            } => {
                let identity =
                    nominal_declaration_identity(&newtype_id, declaration_identity.as_deref());
                let projected = self
                    .get(identity)
                    .filter(|fact| fact.kind == "newtype")
                    .map(|fact| Rc::from(fact.identity.as_str()));
                let (newtype_id, declaration_identity) = match projected {
                    Some(identity) => (identity, None),
                    None => (newtype_id, declaration_identity),
                };
                Value::Newtype {
                    newtype_id,
                    declaration_identity,
                    method_identity,
                    inner: Rc::new(self.project_host_value(Rc::unwrap_or_clone(inner))),
                }
            }
            value => value,
        }
    }
}

impl Machine {
    pub(crate) fn eval_nominal_member(
        &self,
        operation: &Operation,
        receiver: &str,
    ) -> Result<Flow, String> {
        let fact = self.nominals.get(receiver).ok_or_else(|| {
            format!(
                "self target facts have no nominal receiver `{receiver}` for member `{}`",
                operation.detail
            )
        })?;
        if fact.kind != "enum" {
            return Err(format!(
                "self target nominal `{}` is not an enum and has no static member `{}`",
                fact.identity, operation.detail
            ));
        }
        let (variant_index, member) = fact
            .members
            .iter()
            .enumerate()
            .find(|(_, member)| member.name == operation.detail)
            .ok_or_else(|| {
                format!(
                    "self target enum `{}` has no variant `{}`",
                    fact.identity, operation.detail
                )
            })?;
        let variant_index = u32::try_from(variant_index)
            .map_err(|_| format!("self target enum `{}` has too many variants", fact.identity))?;
        if member.arity == 0 {
            return Ok(Flow::Value(RuntimeValue::Data(Value::Enum {
                enum_id: Rc::from(fact.identity.as_str()),
                declaration_identity: None,
                method_identity: None,
                variant: Rc::from(member.name.as_str()),
                variant_index,
                payloads: Rc::from([]),
            })));
        }
        Ok(Flow::Value(RuntimeValue::EnumConstructor {
            identity: fact.identity.clone(),
            variant: member.name.clone(),
            variant_index,
            arity: member.arity,
        }))
    }

    pub(crate) fn construct_enum(
        &self,
        operation: &Operation,
        receiver: &str,
        variant: &str,
        arguments: Vec<Value>,
    ) -> Result<Flow, String> {
        let arguments = arguments
            .into_iter()
            .map(RuntimeValue::Data)
            .collect::<Vec<_>>();
        let fact = self
            .nominals
            .get(receiver)
            .ok_or_else(|| format!("self target facts have no enum `{receiver}`"))?;
        let (variant_index, member) = fact
            .members
            .iter()
            .enumerate()
            .find(|(_, member)| member.name == variant)
            .ok_or_else(|| {
                format!(
                    "self target enum `{}` has no variant `{variant}`",
                    fact.identity
                )
            })?;
        let variant_index = u32::try_from(variant_index)
            .map_err(|_| format!("self target enum `{}` has too many variants", fact.identity))?;
        self.construct_enum_value(
            operation,
            &fact.identity,
            variant,
            variant_index,
            member.arity,
            arguments,
        )
    }

    pub(crate) fn construct_enum_value(
        &self,
        operation: &Operation,
        identity: &str,
        variant: &str,
        variant_index: u32,
        arity: usize,
        arguments: Vec<RuntimeValue>,
    ) -> Result<Flow, String> {
        if arguments.len() != arity {
            return Err(format!(
                "self target enum constructor `{identity}.{variant}` expects {arity} argument(s), found {} at {}",
                arguments.len(),
                operation.id
            ));
        }
        let payloads = arguments
            .into_iter()
            .map(data)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Flow::Value(RuntimeValue::Data(Value::Enum {
            enum_id: Rc::from(identity),
            declaration_identity: None,
            method_identity: None,
            variant: Rc::from(variant),
            variant_index,
            payloads: Rc::from(payloads.into_boxed_slice()),
        })))
    }

    pub(crate) fn construct_nominal(
        &self,
        operation: &Operation,
        name: &str,
        arguments: Vec<RuntimeValue>,
    ) -> Result<Flow, String> {
        let Some(fact) = self.nominals.get(name) else {
            let fields = arguments
                .into_iter()
                .enumerate()
                .map(|(index, value)| (index.to_string(), data(value)))
                .map(|(name, value)| value.map(|value| (name, value)))
                .collect::<Result<Vec<_>, _>>()?;
            return Ok(Flow::Value(RuntimeValue::Data(Value::record(fields))));
        };
        match fact.kind.as_str() {
            "record" => {
                if arguments.len() != fact.members.len() {
                    return Err(format!(
                        "self target record `{}` expects {} field value(s), found {} at {}",
                        fact.identity,
                        fact.members.len(),
                        arguments.len(),
                        operation.id
                    ));
                }
                let fields = fact
                    .members
                    .iter()
                    .zip(arguments)
                    .map(|(member, value)| {
                        data(value).map(|value| (Rc::from(member.name.as_str()), value))
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(Flow::Value(RuntimeValue::Data(Value::nominal_record(
                    &fact.identity,
                    fields,
                ))))
            }
            "newtype" => {
                let mut arguments = arguments;
                if arguments.len() != 1 {
                    return Err(format!(
                        "self target newtype `{}` expects one value, found {} at {}",
                        fact.identity,
                        arguments.len(),
                        operation.id
                    ));
                }
                Ok(Flow::Value(RuntimeValue::Data(Value::newtype(
                    &fact.identity,
                    data(arguments.remove(0))?,
                ))))
            }
            "enum" => Err(format!(
                "self target enum `{}` must be constructed through a variant",
                fact.identity
            )),
            other => Err(format!(
                "self target nominal `{}` has unsupported kind `{other}`",
                fact.identity
            )),
        }
    }

    pub(crate) fn match_pattern<'a>(
        &'a mut self,
        pattern_index: usize,
        value: RuntimeValue,
        environment: Environment,
    ) -> LocalFuture<'a, Result<bool, String>> {
        Box::pin(self.match_pattern_body(pattern_index, value, environment))
    }

    pub(crate) fn function_arity(&self, operation_index: usize) -> (usize, Option<usize>) {
        let parameters = self.program.operations[operation_index]
            .operands
            .iter()
            .filter_map(|operand| {
                matches!(
                    self.program.operations[*operand].kind.as_str(),
                    "binding/parameter" | "binding/variadic-parameter"
                )
                .then_some(&self.program.operations[*operand])
            })
            .collect::<Vec<_>>();
        let variadic = parameters
            .last()
            .is_some_and(|parameter| parameter.kind == "binding/variadic-parameter");
        let fixed = parameters.len() - usize::from(variadic);
        let required = parameters[..fixed]
            .iter()
            .filter(|parameter| parameter.operands.is_empty())
            .count();
        (required, (!variadic).then_some(fixed))
    }

    pub(crate) fn value_callable_arity(&self, value: &Value) -> Option<(usize, Option<usize>)> {
        match value {
            Value::Closure(call) => call
                .as_any()
                .downcast_ref::<ProductClosure>()
                .map(|closure| self.function_arity(closure.operation))
                .or_else(|| Some((call.arity(), Some(call.arity())))),
            Value::Builtin { kind, .. } => Some(kind.arity_range()),
            Value::Composed(pair) => self.value_callable_arity(&pair.0),
            _ => None,
        }
    }

    pub(crate) fn callable_arity(&self, value: &RuntimeValue) -> Option<(usize, Option<usize>)> {
        match value {
            RuntimeValue::Function { operation, .. } => Some(self.function_arity(*operation)),
            RuntimeValue::Data(value) => self.value_callable_arity(value),
            RuntimeValue::Type(name) if matches!(name.as_str(), "Some" | "Ok" | "Err") => {
                Some((1, Some(1)))
            }
            RuntimeValue::Type(name) => {
                self.nominals
                    .get(name)
                    .and_then(|nominal| match nominal.kind.as_str() {
                        "record" => Some((nominal.members.len(), Some(nominal.members.len()))),
                        "newtype" => Some((1, Some(1))),
                        "enum" => None,
                        _ => None,
                    })
            }
            RuntimeValue::EnumConstructor { arity, .. } => Some((*arity, Some(*arity))),
        }
    }

    pub(crate) fn nominal_type_matches(
        &self,
        kind: &str,
        identity: &str,
        arguments: &[SemanticType],
        value: &RuntimeValue,
    ) -> Result<bool, String> {
        let RuntimeValue::Data(value) = value else {
            return Ok(false);
        };
        let fact = self.nominals.get(identity);
        let expected_identity = fact.map_or(identity, |fact| fact.identity.as_str());
        let correct_value_kind = matches!(
            (kind, value),
            ("enum", Value::Enum { .. })
                | ("record", Value::NominalRecord { .. })
                | ("newtype", Value::Newtype { .. })
        );
        if !correct_value_kind
            || value.nominal_declaration_id() != Some(expected_identity)
            || arguments.is_empty()
        {
            return Ok(
                correct_value_kind && value.nominal_declaration_id() == Some(expected_identity)
            );
        }
        let fact = fact
            .ok_or_else(|| format!("typed pattern nominal `{identity}` has no declaration fact"))?;
        if fact.kind != kind {
            return Err(format!(
                "typed pattern nominal `{identity}` expected {kind}, found {}",
                fact.kind
            ));
        }
        if fact.type_parameters.len() != arguments.len() {
            return Err(format!(
                "typed pattern nominal `{identity}` has {} type parameters for {} arguments",
                fact.type_parameters.len(),
                arguments.len()
            ));
        }
        let bindings = fact
            .type_parameters
            .iter()
            .cloned()
            .zip(arguments.iter().cloned())
            .collect::<BTreeMap<_, _>>();
        match (kind, value) {
            ("record", Value::NominalRecord { fields, .. }) => {
                if fields.len() != fact.members.len() {
                    return Ok(false);
                }
                for member in &fact.members {
                    let [member_type] = member.types.as_slice() else {
                        return Err(format!(
                            "typed pattern record `{identity}` field `{}` has no exact type",
                            member.name
                        ));
                    };
                    let Some((_, field_value)) =
                        fields.iter().find(|(name, _)| name.as_ref() == member.name)
                    else {
                        return Ok(false);
                    };
                    let member_type = substitute_semantic_type(member_type, &bindings);
                    if !self.semantic_type_matches(
                        &member_type,
                        &RuntimeValue::Data(field_value.clone()),
                    )? {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
            (
                "enum",
                Value::Enum {
                    variant, payloads, ..
                },
            ) => {
                let Some(member) = fact
                    .members
                    .iter()
                    .find(|member| member.name == variant.as_ref())
                else {
                    return Ok(false);
                };
                if member.types.len() != payloads.len() {
                    return Err(format!(
                        "typed pattern enum `{identity}` variant `{variant}` has {} types for {} payloads",
                        member.types.len(),
                        payloads.len()
                    ));
                }
                for (payload_type, payload) in member.types.iter().zip(payloads.iter()) {
                    let payload_type = substitute_semantic_type(payload_type, &bindings);
                    if !self.semantic_type_matches(
                        &payload_type,
                        &RuntimeValue::Data(payload.clone()),
                    )? {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
            ("newtype", Value::Newtype { inner, .. }) => {
                let base = fact.base_type.as_ref().ok_or_else(|| {
                    format!("typed pattern newtype `{identity}` has no base type")
                })?;
                let base = substitute_semantic_type(base, &bindings);
                self.semantic_type_matches(&base, &RuntimeValue::Data((**inner).clone()))
            }
            _ => Ok(false),
        }
    }

    pub(crate) fn semantic_type_matches(
        &self,
        ty: &SemanticType,
        value: &RuntimeValue,
    ) -> Result<bool, String> {
        let data_value = || match value {
            RuntimeValue::Data(value) => Some(value),
            _ => None,
        };
        Ok(match ty {
            SemanticType::Primitive(primitive) => matches!(
                (primitive, data_value()),
                (SemanticPrimitive::Int, Some(Value::Int(_)))
                    | (SemanticPrimitive::Float, Some(Value::Float(_)))
                    | (SemanticPrimitive::String, Some(Value::Str(_)))
                    | (SemanticPrimitive::Bool, Some(Value::Bool(_)))
                    | (SemanticPrimitive::Unit, Some(Value::Unit))
            ),
            SemanticType::Literal(literal) => match (literal, data_value()) {
                (SemanticLiteral::String(expected), Some(Value::Str(actual))) => {
                    expected == actual.as_ref()
                }
                (SemanticLiteral::Int(expected), Some(Value::Int(actual))) => expected == actual,
                (SemanticLiteral::Float(expected), Some(Value::Float(actual))) => {
                    expected.parse::<f64>() == Ok(*actual)
                }
                (SemanticLiteral::Bool(expected), Some(Value::Bool(actual))) => expected == actual,
                (SemanticLiteral::Null, Some(Value::Null)) => true,
                _ => false,
            },
            SemanticType::Union(members) => {
                for member in members {
                    if self.semantic_type_matches(member, value)? {
                        return Ok(true);
                    }
                }
                false
            }
            SemanticType::Record(fields) => match data_value() {
                Some(Value::Record(values)) if values.len() == fields.len() => {
                    for field in fields {
                        let Some(field_value) = values.get(&field.name) else {
                            return Ok(false);
                        };
                        if !self.semantic_type_matches(
                            &field.ty,
                            &RuntimeValue::Data(field_value.clone()),
                        )? {
                            return Ok(false);
                        }
                    }
                    true
                }
                _ => false,
            },
            SemanticType::Constructor {
                constructor,
                arguments,
            } => match (constructor, arguments.as_slice(), data_value()) {
                (SemanticConstructor::Option, [_], Some(Value::None)) => true,
                (SemanticConstructor::Option, [inner], Some(Value::Some(found))) => {
                    self.semantic_type_matches(inner, &RuntimeValue::Data((**found).clone()))?
                }
                (SemanticConstructor::Result, [_, err], Some(Value::Err(found))) => {
                    self.semantic_type_matches(err, &RuntimeValue::Data((**found).clone()))?
                }
                (SemanticConstructor::Result, [ok, _], Some(Value::Ok(found))) => {
                    self.semantic_type_matches(ok, &RuntimeValue::Data((**found).clone()))?
                }
                (SemanticConstructor::Array, [element], Some(Value::Array(values))) => {
                    for found in values.borrow().iter() {
                        if !self
                            .semantic_type_matches(element, &RuntimeValue::Data(found.clone()))?
                        {
                            return Ok(false);
                        }
                    }
                    true
                }
                (SemanticConstructor::Set, [element], Some(Value::Set(values))) => {
                    for found in values.borrow().items() {
                        if !self.semantic_type_matches(element, &RuntimeValue::Data(found))? {
                            return Ok(false);
                        }
                    }
                    true
                }
                (SemanticConstructor::Map, [key, item], Some(Value::Map(values))) => {
                    for (found_key, found_item) in values.borrow().pairs() {
                        if !self.semantic_type_matches(key, &RuntimeValue::Data(found_key))?
                            || !self.semantic_type_matches(item, &RuntimeValue::Data(found_item))?
                        {
                            return Ok(false);
                        }
                    }
                    true
                }
                (SemanticConstructor::Range, [], Some(Value::Range { .. })) => true,
                _ => false,
            },
            SemanticType::Function {
                parameters,
                variadic,
                ..
            } => {
                let fixed = parameters.len();
                match self.callable_arity(value) {
                    None => false,
                    Some((minimum, maximum)) if variadic.is_some() => {
                        maximum.is_none() && minimum <= fixed
                    }
                    Some((minimum, maximum)) => {
                        minimum <= fixed && maximum.is_none_or(|maximum| fixed <= maximum)
                    }
                }
            }
            SemanticType::Foreign { identity, .. } => data_value()
                .and_then(Value::nominal_declaration_id)
                .is_some_and(|actual| actual == identity),
            SemanticType::Rigid { .. } => true,
            SemanticType::Template => matches!(data_value(), Some(Value::Template(_))),
            SemanticType::File => matches!(data_value(), Some(Value::Resource(_))),
            SemanticType::JsonValue => matches!(data_value(), Some(Value::Json(_))),
            SemanticType::Bytes => matches!(data_value(), Some(Value::Bytes(_))),
            SemanticType::ByteBuffer => matches!(data_value(), Some(Value::ByteBuffer(_))),
            SemanticType::Path => matches!(data_value(), Some(Value::Path(_))),
            SemanticType::Regex => matches!(data_value(), Some(Value::Regex(_))),
            SemanticType::Match => matches!(data_value(), Some(Value::RegexMatch(_))),
            SemanticType::TomlValue => matches!(data_value(), Some(Value::Toml(_))),
            SemanticType::Url => matches!(data_value(), Some(Value::Url(_))),
            SemanticType::Date => matches!(data_value(), Some(Value::Date(_))),
            SemanticType::BigInt => matches!(data_value(), Some(Value::BigInt(_))),
            SemanticType::Decimal => matches!(data_value(), Some(Value::Decimal(_))),
            SemanticType::RoundingMode => matches!(
                data_value(),
                Some(Value::Enum { enum_id, .. }) if enum_id.as_ref() == "RoundingMode"
            ),
            SemanticType::Enum {
                identity,
                arguments,
            } => self.nominal_type_matches("enum", identity, arguments, value)?,
            SemanticType::NominalRecord {
                identity,
                arguments,
            } => self.nominal_type_matches("record", identity, arguments, value)?,
            SemanticType::Newtype {
                identity,
                arguments,
            } => self.nominal_type_matches("newtype", identity, arguments, value)?,
            SemanticType::Unknown | SemanticType::InferenceVariable => {
                return Err("typed pattern retained an incomplete runtime type".to_string());
            }
        })
    }

    pub(crate) async fn match_pattern_body(
        &mut self,
        pattern_index: usize,
        value: RuntimeValue,
        environment: Environment,
    ) -> Result<bool, String> {
        let pattern = self.program.operations[pattern_index].clone();
        match pattern.kind.as_str() {
            "pattern/wildcard" => Ok(true),
            "pattern/binding" | "binding/parameter" | "binding/variadic-parameter" => {
                if pattern.kind == "pattern/binding"
                    && let RuntimeValue::Data(Value::Enum {
                        enum_id,
                        variant,
                        payloads,
                        ..
                    }) = &value
                    && let Some(fact) = self.nominals.get(enum_id.as_ref())
                    && fact.kind == "enum"
                    && fact
                        .members
                        .iter()
                        .any(|member| member.name == pattern.detail && member.arity == 0)
                {
                    return Ok(payloads.is_empty() && variant.as_ref() == pattern.detail);
                }
                self.bind(pattern_index, value, environment)?;
                Ok(true)
            }
            "pattern/typed-binding" => {
                if let Some(ty) = &pattern.pattern_type
                    && !self.semantic_type_matches(ty, &value)?
                {
                    return Ok(false);
                }
                self.bind(pattern_index, value, environment)?;
                Ok(true)
            }
            "pattern/literal" => {
                let expected = data(self.eval_value(pattern.operands[0], environment).await?)?;
                let actual = data(value)?;
                topaz_value::value::values_equal(&expected, &actual)
                    .map_err(|error| format!("{error:?}"))
            }
            "pattern/range" => {
                if pattern.operands.len() != 2 {
                    return Err(format!(
                        "{} range pattern expects two endpoints",
                        pattern.id
                    ));
                }
                let lo = data(
                    self.eval_value(pattern.operands[0], environment.clone())
                        .await?,
                )?;
                let hi = data(self.eval_value(pattern.operands[1], environment).await?)?;
                let (Value::Int(lo), Value::Int(hi), Value::Int(value)) = (lo, hi, data(value)?)
                else {
                    return Err(format!(
                        "{} range-pattern endpoints and value must be int",
                        pattern.id
                    ));
                };
                let inclusive = match pattern.detail.as_str() {
                    "true" => true,
                    "false" => false,
                    other => {
                        return Err(format!(
                            "{} range pattern has invalid inclusive flag `{other}`",
                            pattern.id
                        ));
                    }
                };
                Ok(value >= lo && if inclusive { value <= hi } else { value < hi })
            }
            "pattern/alternatives" => {
                for operand in pattern.operands {
                    let candidate = EnvironmentFrame::child(environment.clone());
                    if self
                        .match_pattern(operand, value.clone(), candidate.clone())
                        .await?
                    {
                        for (key, slot) in candidate.values.borrow().iter() {
                            environment.define(key.clone(), slot.borrow().clone());
                        }
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            "pattern/constructor" => {
                let nominal = self
                    .nominals
                    .operation(&pattern.id)
                    .or_else(|| self.nominals.get(&pattern.reference_identity))
                    .or_else(|| self.nominals.get(&pattern.detail));
                let value = data(value)?;
                let payloads: Vec<RuntimeValue> = match (pattern.detail.as_str(), &value) {
                    ("None", Value::None) => Vec::new(),
                    ("Some", Value::Some(value)) => vec![RuntimeValue::Data((**value).clone())],
                    // `JSON.parseAs<T>` is typed, while this deliberately
                    // small runtime decoder is structural. At a checker-
                    // proven `Option<_>` pattern, a non-null decoded payload
                    // is therefore the erased representation of `Some`.
                    (
                        "Some",
                        value @ (Value::Bool(_)
                        | Value::Int(_)
                        | Value::Float(_)
                        | Value::Str(_)
                        | Value::Bytes(_)
                        | Value::Array(_)
                        | Value::Map(_)
                        | Value::Record(_)
                        | Value::NominalRecord { .. }),
                    ) => vec![RuntimeValue::Data(value.clone())],
                    ("Ok", Value::Ok(value)) => vec![RuntimeValue::Data((**value).clone())],
                    ("Err", Value::Err(value)) => vec![RuntimeValue::Data((**value).clone())],
                    (
                        expected,
                        value @ Value::Enum {
                            variant, payloads, ..
                        },
                    ) if expected == variant.as_ref()
                        && nominal.is_none_or(|fact| {
                            fact.kind == "enum"
                                && value.nominal_declaration_id() == Some(&fact.identity)
                        }) =>
                    {
                        payloads.iter().cloned().map(RuntimeValue::Data).collect()
                    }
                    (_, value @ Value::Newtype { inner, .. })
                        if nominal.is_some_and(|fact| {
                            fact.kind == "newtype"
                                && value.nominal_declaration_id() == Some(&fact.identity)
                        }) =>
                    {
                        vec![RuntimeValue::Data((**inner).clone())]
                    }
                    _ => return Ok(false),
                };
                if payloads.len() != pattern.operands.len() {
                    return Err(format!(
                        "{} constructor pattern expects {} payloads, found {}",
                        pattern.id,
                        pattern.operands.len(),
                        payloads.len()
                    ));
                }
                for (operand, payload) in pattern.operands.into_iter().zip(payloads) {
                    if !self
                        .match_pattern(operand, payload, environment.clone())
                        .await?
                    {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
            "pattern/list" => {
                let Value::Array(values) = data(value)? else {
                    return Ok(false);
                };
                let values = values.borrow().clone();
                let rest = pattern
                    .operand_labels
                    .iter()
                    .enumerate()
                    .filter_map(|(index, label)| list_pattern_rest(label).then_some(index))
                    .collect::<Vec<_>>();
                if rest.len() > 1 {
                    return Err(format!("{} list pattern has multiple rests", pattern.id));
                }
                let Some(rest_index) = rest.first().copied() else {
                    if values.len() != pattern.operands.len() {
                        return Ok(false);
                    }
                    for (operand, value) in pattern.operands.into_iter().zip(values) {
                        if !self
                            .match_pattern(operand, RuntimeValue::Data(value), environment.clone())
                            .await?
                        {
                            return Ok(false);
                        }
                    }
                    return Ok(true);
                };
                let trailing = pattern.operands.len() - rest_index - 1;
                if values.len() < rest_index + trailing {
                    return Ok(false);
                }
                for (operand, value) in pattern.operands[..rest_index]
                    .iter()
                    .copied()
                    .zip(values[..rest_index].iter().cloned())
                {
                    if !self
                        .match_pattern(operand, RuntimeValue::Data(value), environment.clone())
                        .await?
                    {
                        return Ok(false);
                    }
                }
                let suffix_start = values.len() - trailing;
                for (operand, value) in pattern.operands[rest_index + 1..]
                    .iter()
                    .copied()
                    .zip(values[suffix_start..].iter().cloned())
                {
                    if !self
                        .match_pattern(operand, RuntimeValue::Data(value), environment.clone())
                        .await?
                    {
                        return Ok(false);
                    }
                }
                if !self
                    .match_pattern(
                        pattern.operands[rest_index],
                        RuntimeValue::Data(Value::array(values[rest_index..suffix_start].to_vec())),
                        environment,
                    )
                    .await?
                {
                    return Ok(false);
                }
                Ok(true)
            }
            "pattern/record" | "pattern/nominal-record" => {
                let value = data(value)?;
                match (pattern.kind.as_str(), &value) {
                    ("pattern/record", Value::Record(_)) => {}
                    ("pattern/nominal-record", value @ Value::NominalRecord { .. }) => {
                        let expected = self
                            .nominals
                            .operation(&pattern.id)
                            .or_else(|| self.nominals.get(&pattern.reference_identity))
                            .or_else(|| self.nominals.get(&pattern.detail))
                            .ok_or_else(|| {
                                format!(
                                    "{} nominal record pattern has no declaration fact",
                                    pattern.id
                                )
                            })?;
                        if expected.kind != "record"
                            || value.nominal_declaration_id() != Some(&expected.identity)
                        {
                            return Ok(false);
                        }
                    }
                    _ => return Ok(false),
                }
                for (operand, label) in pattern.operands.into_iter().zip(pattern.operand_labels) {
                    let field_name = record_pattern_field_name(&label).ok_or_else(|| {
                        format!("{} record pattern operand has no field name", pattern.id)
                    })?;
                    let field_value = match &value {
                        Value::Record(fields) => fields.get(field_name).cloned(),
                        Value::NominalRecord { fields, .. } => fields
                            .iter()
                            .find(|(candidate, _)| candidate.as_ref() == field_name)
                            .map(|(_, value)| value.clone()),
                        _ => None,
                    };
                    let Some(field_value) = field_value else {
                        return Ok(false);
                    };
                    if !self
                        .match_pattern(
                            operand,
                            RuntimeValue::Data(field_value),
                            environment.clone(),
                        )
                        .await?
                    {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
            other => Err(format!("unsupported Stage 1 pattern `{other}`")),
        }
    }
}
