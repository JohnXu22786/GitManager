//! Ephemeral host selection; never serialized or reconstructed from a receipt.
use super::*;
use std::ffi::OsString;
use std::path::PathBuf;

#[derive(Clone)]
pub(crate) struct RecoveryDestination {
    path: PathBuf,
    parent: Arc<files::Directory>,
    name: OsString,
}
impl RecoveryDestination {
    pub(crate) fn select(path: &Path) -> Result<Self> {
        if !path.is_absolute()
            || path
                .components()
                .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
        {
            return Err(StoreError::Invalid(
                "choose an absolute fresh recovery destination without traversal".into(),
            ));
        }
        let parent = path
            .parent()
            .ok_or_else(|| StoreError::Invalid("recovery needs a parent directory".into()))?;
        let name = path
            .file_name()
            .ok_or_else(|| StoreError::Invalid("recovery needs a new folder name".into()))?
            .to_os_string();
        // No canonicalization of a replaced/link destination. Hold the original
        // no-follow descriptor (and Windows ancestor guards) until publication.
        Ok(Self {
            path: path.into(),
            parent: Arc::new(files::Directory::open(parent)?),
            name,
        })
    }
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
    pub(crate) fn check(&self) -> Result<()> {
        if !self
            .parent
            .same(&files::Directory::open(self.path.parent().unwrap())?)?
        {
            return Err(StoreError::Conflict(
                "the selected recovery parent was replaced; choose its location again".into(),
            ));
        }
        Ok(())
    }
    pub(super) fn checked_parts(&self) -> Result<(Arc<files::Directory>, OsString)> {
        self.check()?;
        Ok((self.parent.clone(), self.name.clone()))
    }
}
