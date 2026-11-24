//! Conservative typed lexical correspondence shared by legacy and decoded
//! source analysis. No alpha-renaming or substitution.
use crate::product_contract::{AppDefinition, AppRef, ContractError, Id, Statement, StatementSlice, StringMapRef, Type};
use std::collections::BTreeMap;

pub(super) fn correspond(
    before: &AppDefinition,
    old: &[Statement],
    after: &AppDefinition,
    new: &[Statement],
    bindings: &BTreeMap<Id, Type>,
) -> Result<bool, ContractError> {
    let mut cx = None;
    let env = super::owned_source_shared::LexicalEnvironment::parameters(
        StringMapRef::Legacy(bindings), false, &mut cx,
    ).map_err(|failure| match failure {
        super::owned_source_shared::SourceFailure::Contract(error) => error,
        super::owned_source_shared::SourceFailure::Allocation(_) => unreachable!("legacy binder trace allocates infallibly"),
    })?;
    super::owned_source_shared::correspond(
        AppRef::Legacy(before), StatementSlice::Legacy(old),
        AppRef::Legacy(after), StatementSlice::Legacy(new),
        &env, &mut cx,
    ).map_err(|failure| match failure {
        super::owned_source_shared::SourceFailure::Contract(error) => error,
        super::owned_source_shared::SourceFailure::Allocation(_) => unreachable!("legacy binder trace allocates infallibly"),
    })
}
