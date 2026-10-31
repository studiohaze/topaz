use super::{environment::EnvironmentFrame, machine::Machine};
use crate::program::{decode_json::substitute_semantic_type, model::*};
use crate::wire::data;
use crate::*;
use topaz_value::value::Schema;

impl Machine {
    /// Use the checked call's explicit target and nominal facts. The shared
    /// value decoder owns validation, path errors, and nominal construction.
    pub(crate) async fn call_json_schema(
        &mut self,
        operation: &Operation,
    ) -> Result<Schema, String> {
        let [target] = operation.call_type_arguments.as_slice() else {
            return Err("typed JSON decoding requires an explicit type argument".into());
        };
        self.json_schema(
            JsonSchemaType {
                ty: target,
                module: Some(&operation.module),
                bindings: &BTreeMap::new(),
            },
            Vec::new(),
        )
        .await
    }

    fn json_schema<'a>(
        &'a mut self,
        target: JsonSchemaType<'a>,
        active: Vec<String>,
    ) -> Pin<Box<dyn Future<Output = Result<Schema, String>> + 'a>> {
        Box::pin(async move {
            Ok(match target.ty {
                SemanticType::Rigid { name, .. } if target.bindings.contains_key(name) => {
                    return self.json_schema(target.bindings[name], active).await;
                }
                SemanticType::Primitive(SemanticPrimitive::Int) => Schema::Int,
                SemanticType::Primitive(SemanticPrimitive::String) => Schema::Str,
                SemanticType::Primitive(SemanticPrimitive::Bool) => Schema::Bool,
                SemanticType::Primitive(SemanticPrimitive::Unit) => Schema::Unit,
                // Literal targets share the scalar decoder used by Stage 0.
                SemanticType::Literal(SemanticLiteral::Int(_)) => Schema::Int,
                SemanticType::Literal(SemanticLiteral::String(_)) => Schema::Str,
                SemanticType::Literal(SemanticLiteral::Bool(_)) => Schema::Bool,
                SemanticType::Literal(SemanticLiteral::Null) => Schema::Null,
                SemanticType::Primitive(SemanticPrimitive::Float)
                | SemanticType::Literal(SemanticLiteral::Float(_)) => {
                    return Err("float is not JSON-decodable".into());
                }
                SemanticType::JsonValue => Schema::Json,
                SemanticType::Constructor {
                    constructor,
                    arguments,
                } => match (constructor, arguments.as_slice()) {
                    (SemanticConstructor::Array, [item]) => Schema::Array(Rc::new(
                        self.json_schema(target.with_type(item), active).await?,
                    )),
                    (SemanticConstructor::Option, [item]) => Schema::Option(Rc::new(
                        self.json_schema(target.with_type(item), active).await?,
                    )),
                    (SemanticConstructor::Map, [key, item]) => {
                        if !matches!(
                            self.json_schema(target.with_type(key), active.clone())
                                .await?,
                            Schema::Str
                        ) {
                            return Err("a Map with a non-string key is not JSON-decodable".into());
                        }
                        Schema::Map(Rc::new(
                            self.json_schema(target.with_type(item), active).await?,
                        ))
                    }
                    _ => return Err("unsupported typed JSON constructor".into()),
                },
                SemanticType::Record(members) => {
                    let mut fields = Vec::new();
                    for member in members {
                        fields.push((
                            Rc::from(member.name.as_str()),
                            self.json_schema(target.with_type(&member.ty), active.clone())
                                .await?,
                            None,
                        ));
                    }
                    Schema::StructRecord {
                        fields: Rc::from(fields),
                    }
                }
                SemanticType::NominalRecord {
                    identity,
                    arguments,
                }
                | SemanticType::Enum {
                    identity,
                    arguments,
                }
                | SemanticType::Newtype {
                    identity,
                    arguments,
                } => {
                    let resolved_identity = json_nominal_identity(identity, target.module);
                    let resolved_arguments = arguments
                        .iter()
                        .map(|argument| target.with_type(argument).qualified())
                        .collect::<Vec<_>>();
                    let key = format!("{resolved_identity}:{resolved_arguments:?}");
                    if active.contains(&key) || active.len() >= 128 {
                        return Err(format!(
                            "JSON_LIMIT: a recursive type `{identity}` has no finite JSON decode schema"
                        ));
                    }
                    let mut active = active;
                    active.push(key);
                    let fact = self
                        .nominals
                        .by_identity
                        .get(resolved_identity.as_ref())
                        .ok_or_else(|| format!("missing JSON nominal `{identity}`"))?
                        .clone();
                    if fact.type_parameters.len() != arguments.len() {
                        return Err(format!(
                            "JSON nominal `{identity}` has inconsistent type arguments"
                        ));
                    }
                    let bindings = fact
                        .type_parameters
                        .iter()
                        .cloned()
                        .zip(arguments.iter().map(|argument| target.with_type(argument)))
                        .collect();
                    let name = Rc::from(fact.name.as_str());
                    let declaration_identity = Some(Rc::from(resolved_identity.as_ref()));
                    let members = JsonSchemaType {
                        ty: target.ty,
                        module: fact.identity.rsplit_once("::").map(|(module, _)| module),
                        bindings: &bindings,
                    };
                    match fact.kind.as_str() {
                        "record" => {
                            let mut fields = Vec::new();
                            for member in &fact.members {
                                let [ty] = member.types.as_slice() else {
                                    return Err("JSON record field has no exact type".into());
                                };
                                let schema = self
                                    .json_schema(members.with_type(ty), active.clone())
                                    .await?;
                                let default = match member
                                    .default_operation_id
                                    .as_deref()
                                    .and_then(|id| self.operation_index(id))
                                {
                                    Some(index) if self.json_constant(index) => {
                                        // Only the same pure constant subset as the Rust
                                        // decoder can supply a missing field default.
                                        self.eval_value(index, EnvironmentFrame::root())
                                            .await
                                            .ok()
                                            .and_then(|value| data(value).ok())
                                    }
                                    _ => None,
                                };
                                fields.push((Rc::from(member.name.as_str()), schema, default));
                            }
                            Schema::Record {
                                record_id: name,
                                declaration_identity,
                                fields: Rc::from(fields),
                            }
                        }
                        "enum" => {
                            let mut variants = Vec::new();
                            for (index, member) in fact.members.iter().enumerate() {
                                let mut payloads = Vec::new();
                                for ty in &member.types {
                                    payloads.push(
                                        self.json_schema(members.with_type(ty), active.clone())
                                            .await?,
                                    );
                                }
                                variants.push((
                                    Rc::from(member.name.as_str()),
                                    index as u32,
                                    Rc::from(payloads),
                                ));
                            }
                            Schema::Enum {
                                enum_id: name,
                                declaration_identity,
                                variants: Rc::from(variants),
                            }
                        }
                        "newtype" => {
                            let base = fact
                                .base_type
                                .as_ref()
                                .ok_or("JSON newtype has no base type")?;
                            Schema::Newtype {
                                newtype_id: name,
                                declaration_identity,
                                base: Rc::new(
                                    self.json_schema(members.with_type(base), active).await?,
                                ),
                            }
                        }
                        _ => return Err("unsupported JSON nominal kind".into()),
                    }
                }
                _ => return Err("unsupported typed JSON target".into()),
            })
        })
    }

    fn json_constant(&self, index: usize) -> bool {
        let op = &self.program.operations[index];
        let literal = matches!(
            op.kind.as_str(),
            "expression/integer"
                | "expression/float"
                | "expression/boolean"
                | "expression/unit"
                | "expression/null"
                | "expression/string"
                | "expression/string-text"
                | "expression/unary"
                | "expression/parenthesized"
        );
        (literal
            || (op.kind == "expression/binary"
                && !matches!(
                    op.detail.as_str(),
                    "And" | "Or" | "Coalesce" | "&&" | "||" | "??"
                )))
            && op.operands.iter().all(|index| self.json_constant(*index))
    }
}

// A type argument retains its use-site scope when a generic nominal's members
// use it. Keeping the original type also preserves identities in diagnostics.
#[derive(Clone, Copy)]
struct JsonSchemaType<'a> {
    ty: &'a SemanticType,
    module: Option<&'a str>,
    bindings: &'a BTreeMap<String, JsonSchemaType<'a>>,
}

impl<'a> JsonSchemaType<'a> {
    fn with_type(self, ty: &'a SemanticType) -> Self {
        Self { ty, ..self }
    }

    // Canonical identities and substituted arguments identify recursive schemas.
    fn qualified(self) -> SemanticType {
        let bindings = self
            .bindings
            .iter()
            .map(|(name, target)| (name.clone(), target.qualified()))
            .collect();
        let mut ty = self.ty.clone();
        qualify_json_type(&mut ty, self.module);
        substitute_semantic_type(&ty, &bindings)
    }
}

fn json_nominal_identity<'a>(identity: &'a str, module: Option<&str>) -> std::borrow::Cow<'a, str> {
    match module.filter(|module| !module.is_empty()) {
        Some(module) if !identity.contains("::") => format!("{module}::{identity}").into(),
        _ => identity.into(),
    }
}

fn qualify_json_type(ty: &mut SemanticType, module: Option<&str>) {
    match ty {
        SemanticType::NominalRecord {
            identity,
            arguments,
        }
        | SemanticType::Enum {
            identity,
            arguments,
        }
        | SemanticType::Newtype {
            identity,
            arguments,
        } => {
            *identity = json_nominal_identity(identity, module).into_owned();
            for argument in arguments {
                qualify_json_type(argument, module);
            }
        }
        SemanticType::Constructor { arguments, .. } => {
            for argument in arguments {
                qualify_json_type(argument, module);
            }
        }
        SemanticType::Record(fields) => {
            for field in fields {
                qualify_json_type(&mut field.ty, module);
            }
        }
        _ => {}
    }
}
