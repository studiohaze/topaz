use super::*;

pub(super) fn function_default_const_shape(expr: &ast::Expr) -> bool {
    match &expr.kind {
        ast::ExprKind::Int
        | ast::ExprKind::Float
        | ast::ExprKind::Bool(_)
        | ast::ExprKind::Null
        | ast::ExprKind::Unit
        | ast::ExprKind::Ident => true,
        ast::ExprKind::String(lit) => {
            lit.tag.is_none()
                && lit
                    .parts
                    .iter()
                    .all(|part| matches!(part, ast::StringPart::Text(_)))
        }
        ast::ExprKind::Paren(inner) => function_default_const_shape(inner),
        ast::ExprKind::Unary { operand, .. } => function_default_const_shape(operand),
        ast::ExprKind::Binary { op, lhs, rhs }
            if !matches!(
                op,
                ast::BinaryOp::And | ast::BinaryOp::Or | ast::BinaryOp::Coalesce
            ) =>
        {
            function_default_const_shape(lhs) && function_default_const_shape(rhs)
        }
        _ => false,
    }
}

impl<'a> ExprChecker<'a> {
    /// §3 (v5.3/v5.4): validate a declared enum's surface. The decl is already
    /// registered nominally by the former (two-phase formation); this enforces the
    /// scope — payload-less, SINGLE-payload (v5.3), and MULTI-payload tuple
    /// variants (v5.4) in a top-level enum. A multi-payload variant is REJECTED
    /// below v5.4. Enum-in-union payloads remain unsupported (rejected), as does
    /// any malformed payload type (from formation).
    pub(super) fn check_enum_decl(&mut self, decl: &'a ast::EnumDecl) {
        let multi_ok = self.former.version() >= LangVersion::V5_4;
        let enum_name = self.former.text(decl.name.span);
        let origins = self.stable_type_parameter_substitutions(&decl.type_params);
        for v in &decl.variants {
            // MULTI-payload tuple variants (2+ types) require v5.4.
            if let Some(tys) = &v.payload
                && tys.len() >= 2
                && !multi_ok
            {
                let vname = self.former.text(v.name.span);
                self.former.error(
                    codes::MALFORMED_TYPE,
                    format!(
                        "enum variant `{vname}` has {} payload types — multi-payload tuple variants need v5.4 (a variant carries at most one payload type before v5.4)",
                        tys.len()
                    ),
                    v.span,
                );
                continue;
            }
            // A payload that is a UNION CONTAINING a user enum (`Wrap(Color | int)`)
            // is unsupported: a bare subpattern `case Wrap(Red)` is ambiguous
            // between an enum-variant match and a binding, which the type-based
            // checker and the value-based runtime/emit resolve differently
            // (run≢build). Reject EACH such payload position cleanly.
            let vname = self.former.text(v.name.span);
            let payload_tys: Vec<Type> = self
                .former
                .enum_info(enum_name)
                .and_then(|i| i.variants.iter().find(|info| info.name == vname))
                .map(|info| info.payloads.clone())
                .unwrap_or_default();
            for (written, formed) in v
                .payload
                .as_deref()
                .unwrap_or_default()
                .iter()
                .zip(&payload_tys)
            {
                let semantic_type = substitute(formed, &origins);
                self.record_typed_node(
                    topaz_hir::TypedNodeKind::Type,
                    written.span,
                    &semantic_type,
                );
            }
            for ty in &payload_tys {
                if union_payload_contains_enum(ty) {
                    self.former.error(
                        codes::MALFORMED_TYPE,
                        format!(
                            "enum variant `{vname}` has a payload `{ty}` that is a union containing an enum — enum-in-union payloads are not supported"
                        ),
                        v.span,
                    );
                }
            }
        }
    }

    /// §3 (v5.4): validate a declared nominal record's surface. The decl is
    /// already registered (the former's two-phase pass); this type-checks each
    /// field's DEFAULT against the field's declared type. A default expression is checked in a scope WITHOUT
    /// the record's own fields bound, so a default may reference globals/imports but
    /// NOT `self` or another field (an unbound name is a TPZ5002).
    pub(super) fn check_record_decl(&mut self, decl: &'a ast::RecordDecl) {
        let env = self.tyenv();
        let origins = self.stable_type_parameter_substitutions(&decl.type_params);
        let record_name = self.former.text(decl.name.span);
        let declared_fields = self
            .former
            .record_info(record_name)
            .map(|info| info.fields.clone())
            .unwrap_or_default();
        for f in &decl.fields {
            let field_name = self.former.text(f.name.span);
            let field_ty = declared_fields
                .iter()
                .find(|field| field.name == field_name)
                .map(|field| field.ty.clone())
                .unwrap_or_else(|| self.former.form(&f.ty, &env));
            let semantic_type = substitute(&field_ty, &origins);
            self.record_typed_node(topaz_hir::TypedNodeKind::Type, f.ty.span, &semantic_type);
            if let Some(default) = &f.default {
                // The default must conform to the field's declared type. It is
                // checked in the current (record-field-free) scope, so it cannot
                // reference sibling fields or `self`.
                self.record_default_depth += 1;
                self.check_expr(default, &field_ty);
                self.record_default_depth -= 1;
            }
        }
    }

    pub(super) fn check_newtype_decl(&mut self, decl: &'a ast::NewtypeDecl) {
        let name = self.former.text(decl.name.span);
        if let Some(base) = self.former.newtype_info(name).map(|info| info.base.clone()) {
            let origins = self.stable_type_parameter_substitutions(&decl.type_params);
            let semantic_type = substitute(&base, &origins);
            self.record_typed_node(
                topaz_hir::TypedNodeKind::Type,
                decl.base.span,
                &semantic_type,
            );
        }
    }

    pub(super) fn enum_construct_info(
        &mut self,
        head: &str,
        ctx: Option<&Type>,
        span: Span,
    ) -> Option<(EnumInfo, String, Vec<Type>)> {
        if let Some(Type::Enum { base, args }) = ctx
            && (nominal_ctx_matches(head, base)
                || self
                    .former
                    .enum_base_for_name(head)
                    .is_some_and(|head_base| &head_base == base))
        {
            let id = nominal_instance_id(base, args);
            return self
                .former
                .enum_info(&id)
                .cloned()
                .map(|info| (info, base.clone(), args.clone()));
        }
        self.former
            .enum_instance(head, Vec::new(), span)
            .map(|info| {
                let base = self
                    .former
                    .enum_base_for_name(head)
                    .unwrap_or_else(|| info.id.clone());
                (info, base, Vec::new())
            })
    }

    pub(super) fn record_construct_info(
        &mut self,
        name: &str,
        ctx: Option<&Type>,
        span: Span,
    ) -> Option<(RecordInfo, String, Vec<Type>)> {
        if let Some(Type::NominalRecord { base, args }) = ctx
            && (nominal_ctx_matches(name, base)
                || self
                    .former
                    .record_base_for_name(name)
                    .is_some_and(|head_base| &head_base == base))
        {
            let id = nominal_instance_id(base, args);
            return self
                .former
                .record_info(&id)
                .cloned()
                .map(|info| (info, base.clone(), args.clone()));
        }
        self.former
            .record_instance(name, Vec::new(), span)
            .map(|info| {
                let base = self
                    .former
                    .record_base_for_name(name)
                    .unwrap_or_else(|| info.id.clone());
                (info, base, Vec::new())
            })
    }

    pub(super) fn newtype_construct_info(
        &mut self,
        head: &str,
        ctx: Option<&Type>,
        span: Span,
    ) -> Option<(NewtypeInfo, String, Vec<Type>)> {
        if let Some(Type::Newtype { base, args }) = ctx
            && (nominal_ctx_matches(head, base)
                || self
                    .former
                    .newtype_base_for_name(head)
                    .is_some_and(|head_base| &head_base == base))
        {
            let id = nominal_instance_id(base, args);
            return self
                .former
                .newtype_info(&id)
                .cloned()
                .map(|info| (info, base.clone(), args.clone()));
        }
        self.former
            .newtype_instance(head, Vec::new(), span)
            .map(|info| {
                let base = self
                    .former
                    .newtype_base_for_name(head)
                    .unwrap_or_else(|| info.id.clone());
                (info, base, Vec::new())
            })
    }

    /// §3 (v5.3/v5.4): type an enum construction `Enum.Variant` (`args` empty),
    /// `Enum.Variant(arg)` (single payload), or `Enum.Variant(a, b, …)` (N-payload,
    /// v5.4). `head` is a declared enum (the caller checked); `field` must name one
    /// of its variants, or it is a TPZ5006 "no such variant". The ARITY must match
    /// the variant's payload arity (`payloads.len()`), and each positional arg is
    /// type-checked POSITION-WISE against the variant's declared payload type.
    /// Named/spread args at a variant constructor are an arity error. The
    /// result retains the nominal `Type::Enum`; a known variant also carries
    /// its exact callable type for typed-call observation.
    pub(super) fn enum_construct(
        &mut self,
        head: &str,
        field: &ast::Ident,
        args: &'a [ast::CallArg],
        span: Span,
        ctx: Option<&Type>,
    ) -> EnumConstruction {
        let variant = self.former.text(field.span).to_string();
        let info = self.enum_construct_info(head, ctx, span);
        let Some((info, base, nominal_args)) = info else {
            self.infer_call_args(args);
            return EnumConstruction {
                result: Type::Unknown,
                callee_type: None,
            };
        };
        let found = info.variants.iter().find(|v| v.name == variant).cloned();
        let result = Type::Enum {
            base,
            args: nominal_args,
        };
        let Some(v) = found else {
            // Still type-check the args so their own errors/effects surface.
            self.infer_call_args(args);
            let known: Vec<&str> = info.variants.iter().map(|v| v.name.as_str()).collect();
            let hint = topaz_diag::suggest::did_you_mean(&variant, known.iter().copied());
            self.former.error(
                codes::NO_FIELD,
                format!("enum `{head}` has no variant `{variant}`{hint}"),
                span,
            );
            return EnumConstruction {
                result,
                callee_type: None,
            };
        };
        let callee_type = Type::Func {
            params: v.payloads.clone(),
            variadic: None,
            ret: Box::new(result.clone()),
        };
        let arity = v.payloads.len();
        // Only positional args participate; named/spread args at a variant
        // constructor are an arity error (type them first for their own errors).
        let positional: Vec<&'a ast::Expr> = args
            .iter()
            .filter_map(|a| match a {
                ast::CallArg::Positional(e) => Some(e),
                _ => None,
            })
            .collect();
        let nonpositional = args.len() - positional.len();
        if positional.len() == arity && nonpositional == 0 {
            // Position-wise type check against each declared payload type.
            for (arg, ty) in positional.iter().zip(v.payloads.iter()) {
                self.check_expr(arg, ty);
            }
        } else {
            self.infer_call_args(args);
            let want = if arity == 0 {
                "no payload".to_string()
            } else {
                let tys: Vec<String> = v.payloads.iter().map(|t| t.to_string()).collect();
                format!(
                    "{arity} payload{} (`{}`)",
                    if arity == 1 { "" } else { "s" },
                    tys.join(", ")
                )
            };
            self.former.error(
                codes::ARITY,
                format!(
                    "enum variant `{head}.{variant}` takes {want}, found {} argument{}",
                    args.len(),
                    if args.len() == 1 { "" } else { "s" }
                ),
                span,
            );
        }
        EnumConstruction {
            result,
            callee_type: Some(callee_type),
        }
    }

    /// §3 (v5.4): type a NEWTYPE CONSTRUCTION `UserId(5)`. `head` is a declared
    /// newtype (the caller checked). Exactly ONE positional argument, type-checked
    /// against the declared base type (TPZ5410-style mismatch via `check_expr`).
    /// Returns the nominal `Type::Newtype` — NEVER the base, so there is no implicit
    /// coercion. Wrong arity / a named/spread arg is an arity error.
    pub(super) fn newtype_construct(
        &mut self,
        head: &str,
        args: &'a [ast::CallArg],
        span: Span,
        ctx: Option<&Type>,
    ) -> Type {
        let info = self.newtype_construct_info(head, ctx, span);
        let Some((info, base_name, nominal_args)) = info else {
            self.infer_call_args(args);
            return Type::Unknown;
        };
        let base = info.base.clone();
        let result = Type::Newtype {
            base: base_name,
            args: nominal_args,
        };
        let positional: Vec<&'a ast::Expr> = args
            .iter()
            .filter_map(|a| match a {
                ast::CallArg::Positional(e) => Some(e),
                _ => None,
            })
            .collect();
        let nonpositional = args.len() - positional.len();
        if positional.len() == 1 && nonpositional == 0 {
            // The wrapped value must match the declared base type exactly — no
            // coercion. `check_expr` reports a mismatch (`expected int, got …`).
            self.check_expr(positional[0], &base);
        } else {
            self.infer_call_args(args);
            self.former.error(
                codes::ARITY,
                format!(
                    "newtype `{head}` constructor takes 1 argument (the `{base}` to wrap), found {}",
                    args.len()
                ),
                span,
            );
        }
        result
    }

    /// §3 (v5.4): type a NOMINAL record CONSTRUCTION `User { name: …, age: … }`,
    /// optionally with a LEADING SPREAD `User { ...spread, … }`. `name` is a
    /// declared record (the caller checked). The SPREAD base must type to the SAME
    /// nominal id (`User { ...u }` ⇒ `u: User`), else TPZ5001; it then supplies
    /// every field. Each EXPLICIT field is type-checked against the declared field
    /// type (overriding the spread); an UNKNOWN field is TPZ5006; a DUPLICATE
    /// explicit field is rejected. A field with NO spread, NO explicit value and NO
    /// default is required (TPZ5004). Returns the nominal `Type::NominalRecord`.
    pub(super) fn nominal_construct(
        &mut self,
        name: &str,
        spread: Option<&'a ast::Expr>,
        fields: &'a [ast::FieldInit],
        span: Span,
        ctx: Option<&Type>,
    ) -> Type {
        let info = self.record_construct_info(name, ctx, span);
        let Some((info, base, nominal_args)) = info else {
            if let Some(spread) = spread {
                self.infer(spread);
            }
            for field in fields {
                self.infer(&field.value);
            }
            return Type::Unknown;
        };
        let result = Type::NominalRecord {
            base,
            args: nominal_args,
        };
        // The spread base (if any) must be the SAME nominal record id — it always
        // evaluates FIRST and supplies a value for EVERY declared field. A
        // wrong-id / non-record base is rejected (the runtime faults identically
        // under `--unchecked`, so check == run).
        let has_spread = if let Some(spread) = spread {
            let spread_ty = self.infer(spread);
            let same = matches!(&spread_ty, Type::NominalRecord { base, args } if nominal_instance_id(base, args) == info.id.as_str());
            if !same && !spread_ty.has_unknown() {
                self.former.error(
                    codes::TYPE_MISMATCH,
                    format!("record spread `...` needs a `{name}`, found `{spread_ty}`"),
                    spread.span,
                );
            }
            true
        } else {
            false
        };
        // Track which declared fields were supplied + reject unknown/dup explicit.
        let mut seen: Vec<&str> = Vec::new();
        for field in fields {
            let fname = self.former.text(field.name.span);
            let decl = info.fields.iter().find(|f| f.name == fname);
            match decl {
                Some(f) => {
                    if seen.contains(&fname) {
                        self.former.error(
                            codes::REDECLARE,
                            format!("field `{fname}` is given twice in `{name}`"),
                            field.span,
                        );
                        self.infer(&field.value);
                    } else {
                        let ty = f.ty.clone();
                        self.check_expr(&field.value, &ty);
                    }
                    seen.push(fname);
                }
                None => {
                    let known: Vec<&str> = info.fields.iter().map(|f| f.name.as_str()).collect();
                    let hint = topaz_diag::suggest::did_you_mean(fname, known.iter().copied());
                    self.former.error(
                        codes::NO_FIELD,
                        format!("record `{name}` has no field `{fname}`{hint}"),
                        field.span,
                    );
                    self.infer(&field.value);
                    seen.push(fname);
                }
            }
        }
        // Require every non-default field to be supplied — UNLESS a spread is
        // present, which supplies a value for every field of the same nominal id.
        let missing: Vec<&str> = if has_spread {
            Vec::new()
        } else {
            info.fields
                .iter()
                .filter(|f| !f.has_default && !seen.contains(&f.name.as_str()))
                .map(|f| f.name.as_str())
                .collect()
        };
        if !missing.is_empty() {
            self.former.error(
                codes::ARITY,
                format!(
                    "record `{name}` is missing field{} {}",
                    if missing.len() == 1 { "" } else { "s" },
                    missing
                        .iter()
                        .map(|m| format!("`{m}`"))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
                span,
            );
        }
        result
    }

    pub(super) fn const_int(&self, e: &ast::Expr) -> Option<i64> {
        match &e.kind {
            ast::ExprKind::Int => self.former.text(e.span).replace('_', "").parse().ok(),
            ast::ExprKind::Paren(inner) => self.const_int(inner),
            ast::ExprKind::Unary { op, operand } => match op {
                ast::UnaryOp::Minus => self.const_int(operand).map(|n| -n),
                ast::UnaryOp::Plus => self.const_int(operand),
                _ => None,
            },
            _ => None,
        }
    }

    pub(super) fn check_function_default_const_expr(&mut self, default: &ast::Expr) {
        if !function_default_const_shape(default) {
            self.former.error(
                codes::TYPE_MISMATCH,
                "`const` initializers must be constant expressions (§4)".to_string(),
                default.span,
            );
            return;
        }
        self.const_fold(default);
    }

    /// §2/§13a: fold a constant INT expression, reporting its arithmetic faults —
    /// division by zero, integer overflow, and a negative or overflowing integer
    /// exponent — as STATIC errors (the same outcomes `run`/`build` reject through
    /// `const_guarded`). Folding BAILS (`None`) on any non-constant-int operand, so
    /// only fully-constant integer arithmetic is judged. Mirrors the shared
    /// `binary_value` int arms; `const` references are not yet resolved (those stay
    /// caught at runtime).
    pub(super) fn const_fold(&mut self, e: &ast::Expr) -> Option<i64> {
        match &e.kind {
            ast::ExprKind::Int => self.former.text(e.span).replace('_', "").parse().ok(),
            ast::ExprKind::Paren(inner) => self.const_fold(inner),
            ast::ExprKind::Unary { op, operand } => {
                let v = self.const_fold(operand)?;
                match op {
                    ast::UnaryOp::Minus => match v.checked_neg() {
                        Some(n) => Some(n),
                        None => {
                            self.const_fault("integer negation overflows", e.span);
                            None
                        }
                    },
                    ast::UnaryOp::Plus => Some(v),
                    _ => None,
                }
            }
            ast::ExprKind::Binary { op, lhs, rhs } => {
                // Match runtime const-eval: the short-circuit operators are not
                // constant expressions, so do NOT descend into their operands (run
                // reports the initializer as non-constant, never an inner fault).
                if matches!(
                    op,
                    ast::BinaryOp::And | ast::BinaryOp::Or | ast::BinaryOp::Coalesce
                ) {
                    return None;
                }
                let l = self.const_fold(lhs)?;
                let r = self.const_fold(rhs)?;
                let res: Result<i64, &str> = match op {
                    ast::BinaryOp::Add => l.checked_add(r).ok_or("integer addition overflows"),
                    ast::BinaryOp::Sub => l.checked_sub(r).ok_or("integer subtraction overflows"),
                    ast::BinaryOp::Mul => {
                        l.checked_mul(r).ok_or("integer multiplication overflows")
                    }
                    ast::BinaryOp::Div => {
                        if r == 0 {
                            Err("integer division by zero")
                        } else {
                            l.checked_div(r).ok_or("integer division overflows")
                        }
                    }
                    ast::BinaryOp::Rem => {
                        if r == 0 {
                            Err("integer remainder by zero")
                        } else {
                            l.checked_rem(r).ok_or("integer remainder overflows")
                        }
                    }
                    ast::BinaryOp::Pow => {
                        if r < 0 {
                            Err("integer exponent must be non-negative; use float operands")
                        } else {
                            match u32::try_from(r) {
                                Ok(exp) => {
                                    l.checked_pow(exp).ok_or("integer exponentiation overflows")
                                }
                                Err(_) => Err("integer exponentiation overflows"),
                            }
                        }
                    }
                    // A comparison/logical operator is not a constant int.
                    _ => return None,
                };
                match res {
                    Ok(v) => Some(v),
                    Err(msg) => {
                        self.const_fault(msg, e.span);
                        None
                    }
                }
            }
            _ => None,
        }
    }

    pub(super) fn const_fault(&mut self, msg: &str, span: Span) {
        self.former.error(
            codes::TYPE_MISMATCH,
            format!("constant expression error: {msg}"),
            span,
        );
    }

    pub(super) fn record_literal(&mut self, fields: &'a [ast::FieldInit]) -> Type {
        let mut formed: Vec<(String, Type)> = Vec::new();
        for field in fields {
            let name = self.former.text(field.name.span).to_string();
            // Field types widen at construction (CDR-004 §4).
            let ty = self.infer(&field.value).widen();
            if formed.iter().any(|(n, _)| *n == name) {
                self.former.error(
                    codes::MALFORMED_TYPE,
                    format!("record literal declares field `{name}` twice"),
                    field.span,
                );
                continue;
            }
            formed.push((name, ty));
        }
        formed.sort_by(|(a, _), (b, _)| a.cmp(b));
        Type::Record(formed)
    }

    pub(super) fn unary_type(&mut self, op: ast::UnaryOp, operand: Type, span: Span) -> Type {
        use ast::UnaryOp::*;
        if operand.has_unknown() {
            return Type::Unknown;
        }
        match (op, &operand) {
            (Not, Type::Prim(Prim::Bool)) => Type::Prim(Prim::Bool),
            (Plus | Minus, Type::Prim(Prim::Int)) => Type::Prim(Prim::Int),
            (Plus | Minus, Type::Prim(Prim::Float)) => Type::Prim(Prim::Float),
            (Plus | Minus, Type::BigInt) => Type::BigInt,
            (Plus | Minus, Type::Decimal) => Type::Decimal,
            _ => {
                self.former.error(
                    codes::TYPE_MISMATCH,
                    format!("unary operator cannot apply to `{operand}`"),
                    span,
                );
                Type::Unknown
            }
        }
    }

    pub(super) fn binary_type(
        &mut self,
        op: ast::BinaryOp,
        lhs: Type,
        rhs: Type,
        span: Span,
    ) -> Type {
        use ast::BinaryOp::*;
        if lhs.has_unknown() || rhs.has_unknown() {
            return Type::Unknown;
        }
        let int = Type::Prim(Prim::Int);
        let float = Type::Prim(Prim::Float);
        let string = Type::Prim(Prim::String);
        let boolean = Type::Prim(Prim::Bool);
        let bigint = Type::BigInt;
        let decimal = Type::Decimal;
        match op {
            Pow | Div => {
                if usable(&lhs, &int) && usable(&rhs, &int) {
                    int
                } else if usable(&lhs, &float) && usable(&rhs, &float) {
                    float
                } else {
                    self.former.error(
                        codes::TYPE_MISMATCH,
                        format!(
                            "arithmetic needs matching numeric operands, found `{lhs}` and `{rhs}`"
                        ),
                        span,
                    );
                    Type::Unknown
                }
            }
            Mul | Sub => {
                if usable(&lhs, &int) && usable(&rhs, &int) {
                    int
                } else if usable(&lhs, &float) && usable(&rhs, &float) {
                    float
                } else if usable(&lhs, &bigint) && usable(&rhs, &bigint) {
                    bigint
                } else if usable(&lhs, &decimal) && usable(&rhs, &decimal) {
                    decimal
                } else {
                    self.former.error(
                        codes::TYPE_MISMATCH,
                        format!(
                            "arithmetic needs matching numeric operands, found `{lhs}` and `{rhs}`"
                        ),
                        span,
                    );
                    Type::Unknown
                }
            }
            // SPEC §2: `%` is int-only in v5.2.
            Rem => {
                if usable(&lhs, &int) && usable(&rhs, &int) {
                    int
                } else {
                    self.former.error(
                        codes::TYPE_MISMATCH,
                        format!("`%` needs int operands in v5.2, found `{lhs}` and `{rhs}`"),
                        span,
                    );
                    Type::Unknown
                }
            }
            Add => {
                if usable(&lhs, &int) && usable(&rhs, &int) {
                    int
                } else if usable(&lhs, &float) && usable(&rhs, &float) {
                    float
                } else if usable(&lhs, &string) && usable(&rhs, &string) {
                    string
                } else if usable(&lhs, &bigint) && usable(&rhs, &bigint) {
                    bigint
                } else if usable(&lhs, &decimal) && usable(&rhs, &decimal) {
                    decimal
                } else {
                    self.former.error(
                        codes::TYPE_MISMATCH,
                        format!(
                            "`+` needs matching int, float, string, BigInt, or Decimal operands, found `{lhs}` and `{rhs}`"
                        ),
                        span,
                    );
                    Type::Unknown
                }
            }
            Lt | Le | Gt | Ge => {
                let ordered_scalar = (usable(&lhs, &int) && usable(&rhs, &int))
                    || (usable(&lhs, &float) && usable(&rhs, &float))
                    || (usable(&lhs, &string) && usable(&rhs, &string))
                    || (usable(&lhs, &bigint) && usable(&rhs, &bigint))
                    || (usable(&lhs, &decimal) && usable(&rhs, &decimal));
                // §3 (v5.4) ORDERING over NOMINAL values (record/enum/newtype) is
                // STRUCTURAL, consistent with `==`: admitted when both operands are
                // ORDER-comparable (every field/payload/base is) AND of the SAME type
                // (mutually subtype-compatible — two distinct nominals are never
                // subtypes, so `P < Q` for different `P`,`Q` still fails here, and a
                // nominal with a non-orderable field/payload is rejected). The runtime
                // `values_compare` leaf decides the order; this gate makes `<` on an
                // order-comparable nominal pass check (no `derives(Order)` required).
                let enums = self.former.enum_table();
                let records = self.former.record_table();
                let newtypes = self.former.newtype_table();
                let ordered_nominal = (is_subtype(&lhs, &rhs) || is_subtype(&rhs, &lhs))
                    && order_comparable_in(&lhs, enums, records, newtypes, &mut Vec::new())
                    && order_comparable_in(&rhs, enums, records, newtypes, &mut Vec::new());
                if ordered_scalar || ordered_nominal {
                    boolean
                } else {
                    self.former.error(
                        codes::INCOMPARABLE,
                        format!("`{lhs}` and `{rhs}` are not ordered comparable"),
                        span,
                    );
                    Type::Unknown
                }
            }
            Eq | Ne => {
                // SPEC §2: Map, Set, functions, files, and templates
                // are non-comparable, recursively through aggregates — and a
                // NOMINAL record/enum/newtype is non-comparable if any declared
                // field/payload/base type is.
                let enums = self.former.enum_table();
                let records = self.former.record_table();
                let newtypes = self.former.newtype_table();
                if !comparable_in(&lhs, enums, records, newtypes, &mut Vec::new())
                    || !comparable_in(&rhs, enums, records, newtypes, &mut Vec::new())
                {
                    self.former.error(
                        codes::INCOMPARABLE,
                        format!("`{lhs}` and `{rhs}` are not comparable values"),
                        span,
                    );
                    Type::Unknown
                } else if is_subtype(&lhs, &rhs) || is_subtype(&rhs, &lhs) {
                    boolean
                } else {
                    self.former.error(
                        codes::INCOMPARABLE,
                        format!("`{lhs}` and `{rhs}` are never equal"),
                        span,
                    );
                    Type::Unknown
                }
            }
            In => match &rhs {
                Type::Ctor(Ctor::Array | Ctor::Set | Ctor::Range, args) => {
                    let elem = args[0].clone();
                    self.expect(&lhs, &elem, span);
                    boolean
                }
                // SPEC §9/§20: membership over a Map is a static
                // error; the canonical form tests its keys.
                Type::Ctor(Ctor::Map, _) => {
                    self.former.error(
                        codes::TYPE_MISMATCH,
                        "`in` does not apply to a Map; use `in map.keys`".to_string(),
                        span,
                    );
                    Type::Unknown
                }
                // SPEC §2 has no substring `in` overload.
                Type::Prim(Prim::String) | Type::Literal(Lit::Str(_)) => {
                    self.former.error(
                        codes::TYPE_MISMATCH,
                        "`in` does not apply to strings".to_string(),
                        span,
                    );
                    Type::Unknown
                }
                _ => Type::Unknown,
            },
            And | Or => {
                self.expect(&lhs, &boolean, span);
                self.expect(&rhs, &boolean, span);
                boolean
            }
            Coalesce => unreachable!("`??` is routed as a context site in infer_with"),
        }
    }
}
