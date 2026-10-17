//! Explicit current-format family dispatch. Scalar payload serialization and
//! identities remain unchanged inside the versioned container envelope.
use super::{compiler, ProjectSnapshot, Result, ScopeState, StoreError, COMPILER_VERSION};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "family",
    content = "state",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum ScopeEnvelope {
    ScalarV1(ScopeState),
    OwnedV2(super::OwnedScopeStateV2),
}

pub(super) enum CurrentScope<'a> {
    ScalarV1(&'a ScopeState),
    OwnedV2(&'a super::OwnedScopeStateV2),
}

impl ProjectSnapshot {
    /// A checked family view is not proof admission or execution authority.
    pub(super) fn current_scope(&self) -> Result<CurrentScope<'_>> {
        if matches!(self.version, 1 | 2) {
            return Err(StoreError::UpgradeRequired);
        }
        if self.version != super::super::FORMAT {
            return Err(StoreError::UnsupportedFormat(self.version));
        }
        match &self.scope {
            ScopeEnvelope::OwnedV2(state) => {
                if state.version != super::OWNED_SCOPE_VERSION {
                    return Err(compiler::error("unsupported owned scope version"));
                }
                Ok(CurrentScope::OwnedV2(state))
            }
            ScopeEnvelope::ScalarV1(state) => {
                if state.version != COMPILER_VERSION {
                    return Err(compiler::error("invalid scope-state version or inventory"));
                }
                Ok(CurrentScope::ScalarV1(state))
            }
        }
    }
    pub(crate) fn scalar_scope(&self) -> Result<&ScopeState> {
        match self.current_scope()? {
            CurrentScope::ScalarV1(state) => Ok(state),
            CurrentScope::OwnedV2(_) => Err(compiler::error(
                "owned family cannot enter scalar scope authority",
            )),
        }
    }
    pub(crate) fn scalar_scope_mut(&mut self) -> Result<&mut ScopeState> {
        self.current_scope()?;
        match &mut self.scope {
            ScopeEnvelope::ScalarV1(state) => Ok(state),
            ScopeEnvelope::OwnedV2(_) => Err(compiler::error(
                "owned family cannot enter scalar scope authority",
            )),
        }
    }
}

impl ProjectSnapshot {
    pub fn owned_scope(&self) -> Result<&super::OwnedScopeStateV2> {
        match self.current_scope()? {
            CurrentScope::OwnedV2(state) => Ok(state),
            CurrentScope::ScalarV1(_) => Err(compiler::error(
                "scalar family cannot enter owned scope authority",
            )),
        }
    }
}
