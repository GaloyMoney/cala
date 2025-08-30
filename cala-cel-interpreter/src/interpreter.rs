use serde::{Deserialize, Serialize};

use cel_parser::{
    ast::{self, ArithmeticOp, Expression, RelationOp},
    parser::ExpressionParser,
};

use crate::{cel_type::*, context::*, error::*, value::*};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(try_from = "String")]
#[serde(into = "String")]
pub struct CelExpression {
    source: String,
    expr: Expression,
}

impl CelExpression {
    pub fn try_evaluate<'a, T: TryFrom<CelResult<'a>, Error = ResultCoercionError>>(
        &'a self,
        ctx: &CelContext,
    ) -> Result<T, CelError> {
        let res = self.evaluate(ctx)?;
        Ok(T::try_from(CelResult {
            expr: &self.expr,
            val: res,
        })?)
    }

    pub fn evaluate(&self, ctx: &CelContext) -> Result<CelValue, CelError> {
        match evaluate_expression(&self.expr, ctx)? {
            EvalType::Value(val) => Ok(val),
            EvalType::ContextItem(ContextItem::Value(val)) => Ok(val.clone()),
            _ => Err(CelError::Unexpected(
                "evaluate didn't return a value".to_string(),
            )),
        }
    }
}

impl std::fmt::Display for CelExpression {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.source)
    }
}

enum EvalType<'a> {
    Value(CelValue),
    ContextItem(&'a ContextItem),
    MemberFn(&'a CelValue, &'a CelMemberFunction),
}

impl EvalType<'_> {
    fn try_into_bool(self) -> Result<bool, CelError> {
        if let EvalType::Value(val) = self {
            val.try_bool()
        } else {
            Err(CelError::Unexpected(
                "Expression didn't resolve to a bool".to_string(),
            ))
        }
    }

    fn try_into_key(self) -> Result<CelKey, CelError> {
        if let EvalType::Value(val) = self {
            match val {
                CelValue::Int(i) => Ok(CelKey::Int(i)),
                CelValue::UInt(u) => Ok(CelKey::UInt(u)),
                CelValue::Bool(b) => Ok(CelKey::Bool(b)),
                CelValue::String(s) => Ok(CelKey::String(s)),
                _ => Err(CelError::Unexpected(
                    "Expression didn't resolve to a valid key".to_string(),
                )),
            }
        } else {
            Err(CelError::Unexpected(
                "Expression didn't resolve to value".to_string(),
            ))
        }
    }

    fn try_into_value(self) -> Result<CelValue, CelError> {
        if let EvalType::Value(val) = self {
            Ok(val)
        } else {
            Err(CelError::Unexpected("Couldn't unwrap value".to_string()))
        }
    }
}

fn evaluate_expression<'a>(
    expr: &Expression,
    ctx: &'a CelContext,
) -> Result<EvalType<'a>, CelError> {
    match evaluate_expression_inner(expr, ctx) {
        Ok(val) => Ok(val),
        Err(e) => Err(CelError::EvaluationError(format!("{expr:?}"), Box::new(e))),
    }
}

fn evaluate_expression_inner<'a>(
    expr: &Expression,
    ctx: &'a CelContext,
) -> Result<EvalType<'a>, CelError> {
    use Expression::*;
    match expr {
        Ternary(cond, left, right) => {
            if evaluate_expression(cond, ctx)?.try_into_bool()? {
                evaluate_expression(left, ctx)
            } else {
                evaluate_expression(right, ctx)
            }
        }
        Member(expr, member) => {
            let ident = evaluate_expression(expr, ctx)?;
            evaluate_member(ident, member, ctx)
        }
        Has(expr) => {
            // The 'has' macro checks if a field exists in a map
            // It expects an expression of the form e.f or e.f.g.h (a Member expression)
            // For nested fields like a.b.c.d, it evaluates a.b.c and checks if 'd' exists

            // Helper function to extract the last field and the target expression
            fn extract_last_field(
                expr: &Expression,
            ) -> Option<(&Expression, &std::sync::Arc<String>)> {
                match expr {
                    Expression::Member(target, member) => match member.as_ref() {
                        ast::Member::Attribute(field_name) => Some((target.as_ref(), field_name)),
                        _ => None,
                    },
                    _ => None,
                }
            }

            if let Some((target_expr, field_name)) = extract_last_field(expr.as_ref()) {
                // Evaluate the target expression (everything except the last field)
                let target = evaluate_expression(target_expr, ctx)?;

                // Check if the field exists in the map
                let has_field = match target {
                    EvalType::Value(CelValue::Map(map)) => map.contains_key(field_name.as_str()),
                    EvalType::ContextItem(ContextItem::Value(CelValue::Map(map))) => {
                        map.contains_key(field_name.as_str())
                    }
                    _ => {
                        // For non-map types, has() should return an error
                        return Err(CelError::IllegalTarget);
                    }
                };

                Ok(EvalType::Value(CelValue::Bool(has_field)))
            } else {
                Err(CelError::Unexpected(
                    "has() expects a member expression".to_string(),
                ))
            }
        }
        Map(entries) => {
            let mut map = CelMap::new();
            for (k, v) in entries {
                let key = evaluate_expression(k, ctx)?;
                let value = evaluate_expression(v, ctx)?;
                map.insert(key.try_into_key()?, value.try_into_value()?)
            }
            Ok(EvalType::Value(CelValue::from(map)))
        }
        Ident(name) => Ok(EvalType::ContextItem(ctx.lookup_ident(name)?)),
        Literal(val) => Ok(EvalType::Value(CelValue::from(val))),
        Arithmetic(op, left, right) => {
            let left = evaluate_expression(left, ctx)?;
            let right = evaluate_expression(right, ctx)?;
            Ok(EvalType::Value(evaluate_arithmetic(
                *op,
                left.try_into_value()?,
                right.try_into_value()?,
            )?))
        }
        Relation(op, left, right) => {
            let left = evaluate_expression(left, ctx)?;
            let right = evaluate_expression(right, ctx)?;
            Ok(EvalType::Value(evaluate_relation(
                *op,
                left.try_into_value()?,
                right.try_into_value()?,
            )?))
        }
        e => Err(CelError::Unexpected(format!("unimplemented {e:?}"))),
    }
}

fn evaluate_member<'a>(
    target: EvalType<'a>,
    member: &ast::Member,
    ctx: &'a CelContext,
) -> Result<EvalType<'a>, CelError> {
    use ast::Member::*;
    match member {
        Attribute(name) => match target {
            EvalType::Value(CelValue::Map(map)) if map.contains_key(name) => {
                Ok(EvalType::Value(map.get(name)))
            }
            EvalType::ContextItem(ContextItem::Value(CelValue::Map(map))) => {
                Ok(EvalType::Value(map.get(name)))
            }
            EvalType::ContextItem(ContextItem::Package(p)) => {
                Ok(EvalType::ContextItem(p.lookup(name)?))
            }
            EvalType::ContextItem(ContextItem::Value(v)) => {
                Ok(EvalType::MemberFn(v, ctx.lookup_member_fn(v, name)?))
            }
            _ => Err(CelError::IllegalTarget),
        },
        FunctionCall(exprs) => match target {
            EvalType::ContextItem(ContextItem::Function(f)) => {
                let mut args = Vec::new();
                for e in exprs {
                    args.push(evaluate_expression(e, ctx)?.try_into_value()?)
                }
                Ok(EvalType::Value(f(args)?))
            }
            EvalType::ContextItem(ContextItem::Package(p)) => {
                evaluate_member(EvalType::ContextItem(p.package_self()?), member, ctx)
            }
            EvalType::MemberFn(v, f) => {
                let mut args = Vec::new();
                for e in exprs {
                    args.push(evaluate_expression(e, ctx)?.try_into_value()?)
                }
                Ok(EvalType::Value(f(v, args)?))
            }
            _ => Err(CelError::IllegalTarget),
        },
        _ => unimplemented!(),
    }
}

fn evaluate_arithmetic(
    op: ArithmeticOp,
    left: CelValue,
    right: CelValue,
) -> Result<CelValue, CelError> {
    use CelValue::*;
    match op {
        ArithmeticOp::Multiply => match (&left, &right) {
            (UInt(l), UInt(r)) => Ok(UInt(l * r)),
            (Int(l), Int(r)) => Ok(Int(l * r)),
            (Double(l), Double(r)) => Ok(Double(l * r)),
            (Decimal(l), Decimal(r)) => Ok(Decimal(l * r)),
            _ => Err(CelError::NoMatchingOverload(format!(
                "Cannot apply '*' to {:?} and {:?}",
                CelType::from(&left),
                CelType::from(&right)
            ))),
        },
        ArithmeticOp::Add => match (&left, &right) {
            (UInt(l), UInt(r)) => Ok(UInt(l + r)),
            (Int(l), Int(r)) => Ok(Int(l + r)),
            (Double(l), Double(r)) => Ok(Double(l + r)),
            (Decimal(l), Decimal(r)) => Ok(Decimal(l + r)),
            _ => Err(CelError::NoMatchingOverload(format!(
                "Cannot apply '+' to {:?} and {:?}",
                CelType::from(&left),
                CelType::from(&right)
            ))),
        },
        ArithmeticOp::Subtract => match (&left, &right) {
            (UInt(l), UInt(r)) => Ok(UInt(l - r)),
            (Int(l), Int(r)) => Ok(Int(l - r)),
            (Double(l), Double(r)) => Ok(Double(l - r)),
            (Decimal(l), Decimal(r)) => Ok(Decimal(l - r)),
            _ => Err(CelError::NoMatchingOverload(format!(
                "Cannot apply '-' to {:?} and {:?}",
                CelType::from(&left),
                CelType::from(&right)
            ))),
        },
        _ => unimplemented!(),
    }
}

fn evaluate_relation(
    op: RelationOp,
    left: CelValue,
    right: CelValue,
) -> Result<CelValue, CelError> {
    use CelValue::*;
    match op {
        RelationOp::LessThan => match (&left, &right) {
            (UInt(l), UInt(r)) => Ok(Bool(l < r)),
            (Int(l), Int(r)) => Ok(Bool(l < r)),
            (Double(l), Double(r)) => Ok(Bool(l < r)),
            (Decimal(l), Decimal(r)) => Ok(Bool(l < r)),
            _ => Err(CelError::NoMatchingOverload(format!(
                "Cannot apply '<' to {:?} and {:?}",
                CelType::from(&left),
                CelType::from(&right)
            ))),
        },
        RelationOp::LessThanEq => match (&left, &right) {
            (UInt(l), UInt(r)) => Ok(Bool(l <= r)),
            (Int(l), Int(r)) => Ok(Bool(l <= r)),
            (Double(l), Double(r)) => Ok(Bool(l <= r)),
            (Decimal(l), Decimal(r)) => Ok(Bool(l <= r)),
            _ => Err(CelError::NoMatchingOverload(format!(
                "Cannot apply '<=' to {:?} and {:?}",
                CelType::from(&left),
                CelType::from(&right)
            ))),
        },
        RelationOp::GreaterThan => match (&left, &right) {
            (UInt(l), UInt(r)) => Ok(Bool(l > r)),
            (Int(l), Int(r)) => Ok(Bool(l > r)),
            (Double(l), Double(r)) => Ok(Bool(l > r)),
            (Decimal(l), Decimal(r)) => Ok(Bool(l > r)),
            _ => Err(CelError::NoMatchingOverload(format!(
                "Cannot apply '>' to {:?} and {:?}",
                CelType::from(&left),
                CelType::from(&right)
            ))),
        },
        RelationOp::GreaterThanEq => match (&left, &right) {
            (UInt(l), UInt(r)) => Ok(Bool(l >= r)),
            (Int(l), Int(r)) => Ok(Bool(l >= r)),
            (Double(l), Double(r)) => Ok(Bool(l >= r)),
            (Decimal(l), Decimal(r)) => Ok(Bool(l >= r)),
            _ => Err(CelError::NoMatchingOverload(format!(
                "Cannot apply '>=' to {:?} and {:?}",
                CelType::from(&left),
                CelType::from(&right)
            ))),
        },
        RelationOp::Equals => match (&left, &right) {
            (UInt(l), UInt(r)) => Ok(Bool(l == r)),
            (Int(l), Int(r)) => Ok(Bool(l == r)),
            (Double(l), Double(r)) => Ok(Bool(l == r)),
            (Decimal(l), Decimal(r)) => Ok(Bool(l == r)),
            _ => Err(CelError::NoMatchingOverload(format!(
                "Cannot apply '==' to {:?} and {:?}",
                CelType::from(&left),
                CelType::from(&right)
            ))),
        },
        RelationOp::NotEquals => match (&left, &right) {
            (UInt(l), UInt(r)) => Ok(Bool(l != r)),
            (Int(l), Int(r)) => Ok(Bool(l != r)),
            (Double(l), Double(r)) => Ok(Bool(l != r)),
            (Decimal(l), Decimal(r)) => Ok(Bool(l != r)),
            _ => Err(CelError::NoMatchingOverload(format!(
                "Cannot apply '!=' to {:?} and {:?}",
                CelType::from(&left),
                CelType::from(&right)
            ))),
        },
        _ => unimplemented!(),
    }
}

impl From<CelExpression> for String {
    fn from(expr: CelExpression) -> Self {
        expr.source
    }
}

impl TryFrom<String> for CelExpression {
    type Error = CelError;

    fn try_from(source: String) -> Result<Self, Self::Error> {
        let expr = ExpressionParser::new()
            .parse(&source)
            .map_err(|e| CelError::CelParseError(e.to_string()))?;
        Ok(Self { source, expr })
    }
}
impl TryFrom<&str> for CelExpression {
    type Error = CelError;

    fn try_from(source: &str) -> Result<Self, Self::Error> {
        Self::try_from(source.to_string())
    }
}
impl std::str::FromStr for CelExpression {
    type Err = CelError;

    fn from_str(source: &str) -> Result<Self, Self::Err> {
        Self::try_from(source.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    #[test]
    fn literals() {
        let expression = "true".parse::<CelExpression>().unwrap();
        let context = CelContext::new();
        assert_eq!(expression.evaluate(&context).unwrap(), CelValue::Bool(true));

        let expression = "1".parse::<CelExpression>().unwrap();
        assert_eq!(expression.evaluate(&context).unwrap(), CelValue::Int(1));

        let expression = "-1".parse::<CelExpression>().unwrap();
        assert_eq!(expression.evaluate(&context).unwrap(), CelValue::Int(-1));

        let expression = "'hello'".parse::<CelExpression>().unwrap();
        assert_eq!(
            expression.evaluate(&context).unwrap(),
            CelValue::String("hello".to_string().into())
        );

        // Tokenizer needs fixing
        // let expression = "1u".parse::<CelExpression>().unwrap();
        // assert_eq!(expression.evaluate(&context).unwrap(), CelValue::UInt(1))
    }

    #[test]
    fn logic() {
        let expression = "true || false ? false && true : true"
            .parse::<CelExpression>()
            .unwrap();
        let context = CelContext::new();
        assert_eq!(
            expression.evaluate(&context).unwrap(),
            CelValue::Bool(false)
        );
        let expression = "true && false ? false : true || false"
            .parse::<CelExpression>()
            .unwrap();
        assert_eq!(expression.evaluate(&context).unwrap(), CelValue::Bool(true))
    }

    #[test]
    fn lookup() {
        let expression = "params.hello.world".parse::<CelExpression>().unwrap();
        let mut hello = CelMap::new();
        hello.insert("world", 42);
        let mut params = CelMap::new();
        params.insert("hello", hello);
        let mut context = CelContext::new();
        context.add_variable("params", params);
        assert_eq!(expression.evaluate(&context).unwrap(), CelValue::Int(42));
    }

    #[test]
    fn to_level_function() {
        let expression = "date('2022-10-10')".parse::<CelExpression>().unwrap();
        let context = CelContext::new();
        assert_eq!(
            expression.evaluate(&context).unwrap(),
            CelValue::Date(NaiveDate::parse_from_str("2022-10-10", "%Y-%m-%d").unwrap())
        );
    }

    #[test]
    fn cast_function() {
        let expression = "decimal('1')".parse::<CelExpression>().unwrap();
        let context = CelContext::new();
        assert_eq!(
            expression.evaluate(&context).unwrap(),
            CelValue::Decimal(1.into())
        );
    }

    #[test]
    fn package_function() -> anyhow::Result<()> {
        let expression = "decimal.Add(decimal('1'), decimal('2'))"
            .parse::<CelExpression>()
            .unwrap();
        let context = CelContext::new();
        assert_eq!(expression.evaluate(&context)?, CelValue::Decimal(3.into()));
        Ok(())
    }

    #[test]
    fn has_macro_with_map() {
        // Test 'has' with existing field
        let expression = "has(params.hello)".parse::<CelExpression>().unwrap();
        let mut params = CelMap::new();
        params.insert("hello", "world");
        let mut context = CelContext::new();
        context.add_variable("params", params);
        assert_eq!(expression.evaluate(&context).unwrap(), CelValue::Bool(true));

        // Test 'has' with non-existing field
        let expression = "has(params.missing)".parse::<CelExpression>().unwrap();
        let mut params = CelMap::new();
        params.insert("hello", "world");
        let mut context = CelContext::new();
        context.add_variable("params", params);
        assert_eq!(
            expression.evaluate(&context).unwrap(),
            CelValue::Bool(false)
        );

        // Test 'has' with nested maps
        let expression = "has(params.nested.field)".parse::<CelExpression>().unwrap();
        let mut nested = CelMap::new();
        nested.insert("field", 42);
        let mut params = CelMap::new();
        params.insert("nested", nested);
        let mut context = CelContext::new();
        context.add_variable("params", params);
        assert_eq!(expression.evaluate(&context).unwrap(), CelValue::Bool(true));

        // Test 'has' with deeply nested maps (a.b.c.d)
        let expression = "has(config.database.settings.maxConnections)"
            .parse::<CelExpression>()
            .unwrap();
        let mut settings = CelMap::new();
        settings.insert("maxConnections", 100);
        settings.insert("timeout", 30);
        let mut database = CelMap::new();
        database.insert("settings", settings);
        let mut config = CelMap::new();
        config.insert("database", database);
        let mut context = CelContext::new();
        context.add_variable("config", config);
        assert_eq!(expression.evaluate(&context).unwrap(), CelValue::Bool(true));

        // Test 'has' with deeply nested maps - missing final field
        let expression = "has(config.database.settings.missingField)"
            .parse::<CelExpression>()
            .unwrap();
        let mut settings = CelMap::new();
        settings.insert("maxConnections", 100);
        let mut database = CelMap::new();
        database.insert("settings", settings);
        let mut config = CelMap::new();
        config.insert("database", database);
        let mut context = CelContext::new();
        context.add_variable("config", config);
        assert_eq!(
            expression.evaluate(&context).unwrap(),
            CelValue::Bool(false)
        );
    }

    #[test]
    fn function_on_timestamp() -> anyhow::Result<()> {
        use chrono::{DateTime, Utc};

        let time: DateTime<Utc> = "1940-12-21T00:00:00Z".parse().unwrap();
        let mut context = CelContext::new();
        context.add_variable("now", time);

        let expression = "now.format('%d/%m/%Y')".parse::<CelExpression>().unwrap();
        assert_eq!(expression.evaluate(&context)?, CelValue::from("21/12/1940"));

        Ok(())
    }
}
