use proc_macro2::Span;
use syn::{
    parse::{Parse, ParseStream},
    punctuated::Punctuated,
};

pub struct QueryInput {
    pub(super) ignore_prefix: Option<String>,
    pub(super) executor: syn::Expr,
    pub(super) sql: String,
    pub(super) sql_span: Span,
    pub(super) arg_exprs: Vec<syn::Expr>,
}

impl QueryInput {
    pub(super) fn table_name(&self) -> darling::Result<String> {
        let query = self.sql.to_lowercase();
        let words: Vec<&str> = query.split_whitespace().collect();
        let from_pos = words.iter().position(|&word| word == "from").ok_or(
            darling::Error::custom("Could not identify table name - no 'FROM' clause")
                .with_span(&self.sql_span),
        )?;
        let table_name = words.get(from_pos + 1).ok_or(
            darling::Error::custom("No word after 'FROM' clause").with_span(&self.sql_span),
        )?;
        let table_name = table_name.trim_end_matches(|c: char| !c.is_alphanumeric());
        Ok(table_name.to_string())
    }

    pub(super) fn table_name_without_prefix(&self) -> darling::Result<String> {
        let table_name = self.table_name()?;
        if let Some(ignore_prefix) = &self.ignore_prefix {
            if table_name.starts_with(ignore_prefix) {
                return Ok(table_name[ignore_prefix.len() + 1..].to_string());
            }
        }
        Ok(table_name)
    }

    pub(super) fn order_by(&self) -> String {
        let columns = self.order_by_columns();
        if columns.is_empty() {
            "i.id,".to_string()
        } else {
            columns.join(", ") + ", i.id,"
        }
    }

    fn order_by_columns(&self) -> Vec<String> {
        use regex::Regex;
        let re = Regex::new(r"(?i)ORDER\s+BY\s+(.+?)(?:\s+(?:LIMIT|OFFSET)|\s*;?\s*$)").unwrap();

        if let Some(captures) = re.captures(&self.sql.to_lowercase()) {
            if let Some(order_by_clause) = captures.get(1) {
                return order_by_clause
                    .as_str()
                    .split(',')
                    .map(|s| format!("i.{}", s.trim()))
                    .filter(|s| !s.is_empty())
                    .collect();
            }
        }

        Vec::new()
    }
}

impl Parse for QueryInput {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut sql: Option<(String, Span)> = None;
        let mut args: Option<Vec<syn::Expr>> = None;
        let mut executor: Option<syn::Expr> = None;
        let mut expect_comma = false;
        let mut ignore_prefix = None;

        while !input.is_empty() {
            if expect_comma {
                let _ = input.parse::<syn::token::Comma>()?;
            }
            let key: syn::Ident = input.parse()?;

            let _ = input.parse::<syn::token::Eq>()?;

            if key == "executor" {
                executor = Some(input.parse::<syn::Expr>()?);
            } else if key == "ignore_prefix" {
                ignore_prefix = Some(input.parse::<syn::LitStr>()?.value());
            } else if key == "sql" {
                sql = Some((
                    Punctuated::<syn::LitStr, syn::Token![+]>::parse_separated_nonempty(input)?
                        .iter()
                        .map(syn::LitStr::value)
                        .collect(),
                    input.span(),
                ));
            } else if key == "args" {
                let exprs = input.parse::<syn::ExprArray>()?;
                args = Some(exprs.elems.into_iter().collect())
            } else {
                let message = format!("unexpected input key: {key}");
                return Err(syn::Error::new_spanned(key, message));
            }

            expect_comma = true;
        }

        let (sql, sql_span) = sql.ok_or_else(|| input.error("expected `sql` key"))?;
        let executor = executor.ok_or_else(|| input.error("expected `executor` key"))?;

        Ok(QueryInput {
            ignore_prefix,
            executor,
            sql,
            sql_span,
            arg_exprs: args.unwrap_or_default(),
        })
    }
}

#[cfg(test)]
mod tests {
    use syn::parse_quote;

    use super::*;

    #[test]
    fn parse_input() {
        let input: QueryInput = parse_quote!(
            ignore_prefix = "ignore_prefix",
            executor = &mut **tx,
            sql = "SELECT * FROM ignore_prefix_users WHERE name = $1",
            args = [id]
        );
        assert_eq!(input.ignore_prefix, Some("ignore_prefix".to_string()));
        assert_eq!(
            input.sql,
            "SELECT * FROM ignore_prefix_users WHERE name = $1"
        );
        assert_eq!(input.executor, parse_quote!(&mut **tx));
        assert_eq!(input.arg_exprs[0], parse_quote!(id));
        assert_eq!(input.table_name_without_prefix().unwrap(), "users");
    }

    #[test]
    fn test_order_by_columns() {
        let test_cases = vec![
            (
                "SELECT id FROM entities WHERE (id > $2) OR $2 IS NULL ORDER BY id LIMIT $1",
                vec!["i.id"],
            ),
            (
                "select id from entities order by name asc, date desc",
                vec!["i.name asc", "i.date desc"],
            ),
            ("SELECT TOP 10 id FROM entities Order By id", vec!["i.id"]),
            (
                "select id from entities ORDER BY id offset 10",
                vec!["i.id"],
            ),
            ("SELECT id FROM entities orDer bY id;", vec!["i.id"]),
            (
                "SELECT * FROM users WHERE age > 18 ORDER BY last_name, first_name DESC LIMIT 10",
                vec!["i.last_name", "i.first_name desc"],
            ),
            (
                "SELECT * FROM products ORDER BY price ASC, stock DESC, name",
                vec!["i.price asc", "i.stock desc", "i.name"],
            ),
            ("SELECT * FROM orders", vec![]),
            (
                "SELECT * FROM orders ORDER BY orders NULLS FIRST, id",
                vec!["i.orders nulls first", "i.id"],
            ),
        ];

        for (sql, expected) in test_cases {
            let input = QueryInput {
                ignore_prefix: None,
                executor: parse_quote!(&mut **tx),
                sql: sql.to_string(),
                sql_span: Span::call_site(),
                arg_exprs: vec![],
            };
            assert_eq!(
                input.order_by_columns(),
                expected,
                "Failed for SQL: {}",
                sql
            );
        }
    }
}
