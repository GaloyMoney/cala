use std::sync::Arc;

#[derive(Debug, Eq, PartialEq, Clone)]
pub enum LogicOp {
    And,
    Or,
}

#[derive(Debug, Eq, PartialEq, Clone, Copy)]
pub enum RelationOp {
    LessThan,
    LessThanEq,
    GreaterThan,
    GreaterThanEq,
    Equals,
    NotEquals,
    In,
}

#[derive(Debug, Eq, PartialEq, Clone, Copy)]
pub enum ArithmeticOp {
    Add,
    Subtract,
    Divide,
    Multiply,
    Modulus,
}

#[derive(Debug, Eq, PartialEq, Clone)]
pub enum UnaryOp {
    Not,
    DoubleNot,
    Minus,
    DoubleMinus,
}

#[derive(Debug, Eq, PartialEq, Clone)]
pub enum LeftRightOp {
    Logic(LogicOp),
    Relation(RelationOp),
    Arithmetic(ArithmeticOp),
}

#[derive(Debug, PartialEq, Clone)]
pub enum Expression {
    Ternary(Box<Expression>, Box<Expression>, Box<Expression>),
    Relation(RelationOp, Box<Expression>, Box<Expression>),
    Arithmetic(ArithmeticOp, Box<Expression>, Box<Expression>),
    Unary(UnaryOp, Box<Expression>),

    Member(Box<Expression>, Box<Member>),
    Has(Box<Expression>),

    List(Vec<Expression>),
    Map(Vec<(Expression, Expression)>),
    Struct(Vec<Arc<String>>, Vec<(Arc<String>, Expression)>),

    Literal(Literal),
    Ident(Arc<String>),
}

impl Expression {
    pub(crate) fn from_op(op: LeftRightOp, left: Box<Expression>, right: Box<Expression>) -> Self {
        use LeftRightOp::*;
        match op {
            Logic(LogicOp::Or) => Expression::Ternary(
                left,
                Box::new(Expression::Literal(Literal::Bool(true))),
                right,
            ),
            Logic(LogicOp::And) => Expression::Ternary(
                left,
                right,
                Box::new(Expression::Literal(Literal::Bool(false))),
            ),
            Relation(op) => Expression::Relation(op, left, right),
            Arithmetic(op) => Expression::Arithmetic(op, left, right),
        }
    }
}

#[derive(Debug, PartialEq, Clone)]
pub enum Member {
    Attribute(Arc<String>),
    FunctionCall(Vec<Expression>),
    Index(Box<Expression>),
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum Literal {
    Int(i64),
    UInt(u64),
    Double(Arc<String>),
    String(Arc<String>),
    Bytes(Arc<Vec<u8>>),
    Bool(bool),
    Null,
}

#[cfg(test)]
mod tests {
    use crate::{
        parse_expression, ArithmeticOp::*, Expression, Expression::*, Literal::*, Member::*,
    };

    fn parse(input: &str) -> Expression {
        parse_expression(input.to_string()).unwrap_or_else(|e| panic!("{}", e))
    }

    fn assert_parse_eq(input: &str, expected: Expression) {
        assert_eq!(parse(input), expected);
    }

    #[test]
    fn op_precedence() {
        assert_parse_eq(
            "1 + 2 * 3",
            Arithmetic(
                Add,
                Literal(Int(1)).into(),
                Arithmetic(Multiply, Literal(Int(2)).into(), Literal(Int(3)).into()).into(),
            ),
        );
        assert_parse_eq(
            "1 * 2 + 3",
            Arithmetic(
                Add,
                Arithmetic(Multiply, Literal(Int(1)).into(), Literal(Int(2)).into()).into(),
                Literal(Int(3)).into(),
            ),
        );
        assert_parse_eq(
            "1 * (2 + 3)",
            Arithmetic(
                Multiply,
                Literal(Int(1)).into(),
                Arithmetic(Add, Literal(Int(2)).into(), Literal(Int(3)).into()).into(),
            ),
        )
    }

    #[test]
    fn simple_int() {
        assert_parse_eq("1", Literal(Int(1)))
    }

    #[test]
    fn simple_float() {
        assert_parse_eq("1.0", Literal(Double("1.0".to_string().into())))
    }

    #[test]
    fn lookup() {
        assert_parse_eq(
            "hello.world",
            Member(
                Ident("hello".to_string().into()).into(),
                Attribute("world".to_string().into()).into(),
            ),
        )
    }

    #[test]
    fn nested_attributes() {
        assert_parse_eq(
            "a.b[1]",
            Member(
                Member(
                    Ident("a".to_string().into()).into(),
                    Attribute("b".to_string().into()).into(),
                )
                .into(),
                Index(Literal(Int(1)).into()).into(),
            ),
        )
    }

    #[test]
    fn has_macro() {
        assert_parse_eq(
            "has(a.b)",
            Has(Member(
                Ident("a".to_string().into()).into(),
                Attribute("b".to_string().into()).into(),
            )
            .into()),
        );
        assert_parse_eq(
            "has(params.field)",
            Has(Member(
                Ident("params".to_string().into()).into(),
                Attribute("field".to_string().into()).into(),
            )
            .into()),
        );
        // Test deeply nested has expression
        assert_parse_eq(
            "has(a.b.c.d)",
            Has(Member(
                Member(
                    Member(
                        Ident("a".to_string().into()).into(),
                        Attribute("b".to_string().into()).into(),
                    )
                    .into(),
                    Attribute("c".to_string().into()).into(),
                )
                .into(),
                Attribute("d".to_string().into()).into(),
            )
            .into()),
        );
    }
}
