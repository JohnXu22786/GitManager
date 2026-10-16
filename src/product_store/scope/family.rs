//! Transient dispatch for the supported current scope family. This view is not
//! serialized: format 2 still stores the exact original scalar ScopeState.
use super::{compiler, ProjectSnapshot, Result, ScopeState, StoreError, COMPILER_VERSION};

pub(super) enum CurrentScope<'a> {
    ScalarV1(&'a ScopeState),
}

impl ProjectSnapshot {
    /// Check only container/family versions. Full proof admission still belongs
    /// to snapshot validation; obtaining this view grants no execution authority.
    pub(super) fn current_scope(&self) -> Result<CurrentScope<'_>> {
        if self.version != super::super::FORMAT {
            return Err(StoreError::UnsupportedFormat(self.version));
        }
        if self.scope.version != COMPILER_VERSION {
            return Err(compiler::error("invalid scope-state version or inventory"));
        }
        Ok(CurrentScope::ScalarV1(&self.scope))
    }

    /// Borrow the exact scalar payload, without wrapping any identity input or
    /// substituting empty state for an unsupported family/version.
    pub(crate) fn scalar_scope(&self) -> Result<&ScopeState> {
        match self.current_scope()? {
            CurrentScope::ScalarV1(state) => Ok(state),
        }
    }

    pub(crate) fn scalar_scope_mut(&mut self) -> Result<&mut ScopeState> {
        match self.current_scope()? {
            CurrentScope::ScalarV1(_) => Ok(&mut self.scope),
        }
    }
}
