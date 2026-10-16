//! Conservative typed lexical correspondence. No alpha-renaming or substitution.
use crate::product_contract::*;
use std::collections::BTreeMap;

type Environment = BTreeMap<Id, Type>;
type Result<T> = std::result::Result<T, ContractError>;
#[derive(PartialEq, Eq)]
struct Occurrence {
    kind: &'static str,
    name: Id,
    value_type: Type,
    environment: Environment,
}
struct Trace<'a> {
    app: &'a AppDefinition,
    occurrences: Vec<Occurrence>,
}
impl Trace<'_> {
    fn bind(
        &mut self,
        kind: &'static str,
        name: &str,
        value_type: Type,
        env: &Environment,
    ) -> Environment {
        self.occurrences.push(Occurrence {
            kind,
            name: name.into(),
            value_type: value_type.clone(),
            environment: env.clone(),
        });
        let mut local = env.clone();
        local.insert(name.into(), value_type);
        local
    }
    fn item(&self, expression: &Expr, env: &Environment) -> Result<Type> {
        match self.app.expression_type_in(expression, env)? {
            Type::List { item } => Ok(*item),
            _ => Err(ContractError(
                "Owned source structure: untyped binder collection".into(),
            )),
        }
    }
    fn expression(&mut self, expression: &Expr, env: &Environment) -> Result<()> {
        match expression {
            Expr::Query {
                entity,
                binding,
                predicate,
                sort,
                ..
            } => {
                let local = self.bind("query", binding, Type::reference(entity), env);
                self.expression(predicate, &local)?;
                for key in sort {
                    self.expression(&key.value, &local)?;
                }
            }
            Expr::Filter {
                items,
                binding,
                predicate,
            }
            | Expr::Any {
                items,
                binding,
                predicate,
            }
            | Expr::All {
                items,
                binding,
                predicate,
            } => {
                self.expression(items, env)?;
                let kind = match expression {
                    Expr::Filter { .. } => "filter",
                    Expr::Any { .. } => "any",
                    _ => "all",
                };
                let local = self.bind(kind, binding, self.item(items, env)?, env);
                self.expression(predicate, &local)?;
            }
            Expr::Map {
                items,
                binding,
                value,
            } => {
                self.expression(items, env)?;
                let local = self.bind("map", binding, self.item(items, env)?, env);
                self.expression(value, &local)?;
            }
            Expr::Field { record, .. } => self.expression(record, env)?,
            Expr::Count { items } | Expr::Sum { items } => self.expression(items, env)?,
            Expr::Contains { items, value } => {
                self.expression(items, env)?;
                self.expression(value, env)?;
            }
            Expr::Equal { left, right }
            | Expr::Less { left, right }
            | Expr::Add { left, right }
            | Expr::Subtract { left, right }
            | Expr::Concat { left, right } => {
                self.expression(left, env)?;
                self.expression(right, env)?;
            }
            Expr::TextContains { text, search } => {
                self.expression(text, env)?;
                self.expression(search, env)?;
            }
            Expr::Lower { value } | Expr::Not { value } => self.expression(value, env)?,
            Expr::And { values } | Expr::Or { values } => {
                for value in values {
                    self.expression(value, env)?;
                }
            }
            Expr::If {
                condition,
                then_value,
                else_value,
            } => {
                self.expression(condition, env)?;
                self.expression(then_value, env)?;
                self.expression(else_value, env)?;
            }
            Expr::Coalesce { value, fallback } => {
                self.expression(value, env)?;
                self.expression(fallback, env)?;
            }
            Expr::DateAdd { date, days } => {
                self.expression(date, env)?;
                self.expression(days, env)?;
            }
            Expr::DateDifference { later, earlier } => {
                self.expression(later, env)?;
                self.expression(earlier, env)?;
            }
            Expr::Literal { .. } | Expr::Variable { .. } | Expr::State { .. } | Expr::Today => {}
        }
        Ok(())
    }
    fn statements(&mut self, statements: &[Statement], env: &mut Environment) -> Result<()> {
        for statement in statements {
            match statement {
                Statement::Create {
                    entity,
                    values,
                    bind,
                } => {
                    for value in values.values() {
                        self.expression(value, env)?;
                    }
                    *env = self.bind("create", bind, Type::reference(entity), env);
                }
                Statement::Update { record, values } => {
                    self.expression(record, env)?;
                    for value in values.values() {
                        self.expression(value, env)?;
                    }
                }
                Statement::Archive { record } => self.expression(record, env)?,
                Statement::SetState { value, .. } => self.expression(value, env)?,
                Statement::Collection { target, items, .. } => {
                    if let CollectionTarget::Field { record, .. } = target {
                        self.expression(record, env)?;
                    }
                    self.expression(items, env)?;
                }
                Statement::ForEach {
                    items,
                    binding,
                    steps,
                } => {
                    self.expression(items, env)?;
                    let mut local = self.bind("for_each", binding, self.item(items, env)?, env);
                    self.statements(steps, &mut local)?;
                }
                Statement::Assert { condition, .. } => self.expression(condition, env)?,
                Statement::Emit {
                    items,
                    binding,
                    columns,
                    ..
                } => {
                    self.expression(items, env)?;
                    let local = self.bind("emit", binding, self.item(items, env)?, env);
                    for value in columns.values() {
                        self.expression(value, &local)?;
                    }
                }
            }
        }
        Ok(())
    }
}

/// The first release pairs every existing lexical binder occurrence by order,
/// kind, name, inferred type, and complete entering lexical environment. New
/// occurrences may follow that original prefix. Other insertion/reordering is
/// unsupported; callers can split statement edits at explicit anchored gaps.
/// This preserves multiplicity and sibling/nested scope without guessing an
/// alpha-equivalence or treating a bag of reused names as correspondence.
pub(super) fn correspond(
    before: &AppDefinition,
    old: &[Statement],
    after: &AppDefinition,
    new: &[Statement],
    env: &Environment,
) -> Result<bool> {
    let mut old_trace = Trace {
        app: before,
        occurrences: vec![],
    };
    old_trace.statements(old, &mut env.clone())?;
    let mut new_trace = Trace {
        app: after,
        occurrences: vec![],
    };
    new_trace.statements(new, &mut env.clone())?;
    Ok(new_trace.occurrences.starts_with(&old_trace.occurrences))
}
