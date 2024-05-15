use serde::{Deserialize, Serialize};

use cel_parser::{
    ast::{self, ArithmeticOp, Expression, RelationOp},
    parser::ExpressionParser,
};

use std::sync::Arc;

use crate::{cel_type::*, context::*, error::*, value::*};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(try_from = "String")]
#[serde(into = "String")]
pub struct CelExpression {
    source: String,
    expr: Expression,
}

impl CelExpression {
    pub fn try_evaluate<'a, T: TryFrom<CelResult<'a>, Error = ResultCoersionError>>(
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
}

impl<'a> EvalType<'a> {
    fn try_bool(&self) -> Result<bool, CelError> {
        if let EvalType::Value(val) = self {
            val.try_bool()
        } else {
            Err(CelError::Unexpected(
                "Expression didn't resolve to a bool".to_string(),
            ))
        }
    }

    fn try_key(&self) -> Result<CelKey, CelError> {
        if let EvalType::Value(val) = self {
            match val {
                CelValue::Int(i) => Ok(CelKey::Int(*i)),
                CelValue::UInt(u) => Ok(CelKey::UInt(*u)),
                CelValue::Bool(b) => Ok(CelKey::Bool(*b)),
                CelValue::String(s) => Ok(CelKey::String(s.clone())),
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

    fn try_value(&self) -> Result<CelValue, CelError> {
        if let EvalType::Value(val) = self {
            Ok(val.clone())
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
            if evaluate_expression(cond, ctx)?.try_bool()? {
                evaluate_expression(left, ctx)
            } else {
                evaluate_expression(right, ctx)
            }
        }
        Member(expr, member) => {
            let ident = evaluate_expression(expr, ctx)?;
            evaluate_member(ident, member, ctx)
        }
        Map(entries) => {
            let mut map = CelMap::new();
            for (k, v) in entries {
                let key = evaluate_expression(k, ctx)?;
                let value = evaluate_expression(v, ctx)?;
                map.insert(key.try_key()?, value.try_value()?)
            }
            Ok(EvalType::Value(CelValue::from(map)))
        }
        Ident(name) => Ok(EvalType::ContextItem(ctx.lookup(Arc::clone(name))?)),
        Literal(val) => Ok(EvalType::Value(CelValue::from(val))),
        Arithmetic(op, left, right) => {
            let left = evaluate_expression(left, ctx)?;
            let right = evaluate_expression(right, ctx)?;
            Ok(EvalType::Value(evaluate_arithmetic(
                *op,
                left.try_value()?,
                right.try_value()?,
            )?))
        }
        Relation(op, left, right) => {
            let left = evaluate_expression(left, ctx)?;
            let right = evaluate_expression(right, ctx)?;
            Ok(EvalType::Value(evaluate_relation(
                *op,
                left.try_value()?,
                right.try_value()?,
            )?))
        }
        e => Err(CelError::Unexpected(format!("unimplemented {e:?}"))),
    }
}

fn evaluate_member<'a>(
    target: EvalType,
    member: &ast::Member,
    ctx: &CelContext,
) -> Result<EvalType<'a>, CelError> {
    use ast::Member::*;
    match member {
        Attribute(name) => match target {
            EvalType::ContextItem(ContextItem::Value(CelValue::Map(map))) => {
                Ok(EvalType::Value(map.get(name)))
            }
            _ => Err(CelError::IllegalTarget),
        },
        FunctionCall(exprs) => match target {
            EvalType::ContextItem(ContextItem::Function(f)) => {
                let mut args = Vec::new();
                for e in exprs {
                    args.push(evaluate_expression(e, ctx)?.try_value()?)
                }
                Ok(EvalType::Value(f(args)?))
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
        let expression = "params.hello".parse::<CelExpression>().unwrap();
        let mut context = CelContext::new();
        let mut params = CelMap::new();
        params.insert("hello", 42);
        context.add_variable("params", params);
        assert_eq!(expression.evaluate(&context).unwrap(), CelValue::Int(42));
    }

    #[test]
    fn function() {
        let expression = "date('2022-10-10')".parse::<CelExpression>().unwrap();
        let context = CelContext::new();
        assert_eq!(
            expression.evaluate(&context).unwrap(),
            CelValue::Date(NaiveDate::parse_from_str("2022-10-10", "%Y-%m-%d").unwrap())
        );
    }
}
