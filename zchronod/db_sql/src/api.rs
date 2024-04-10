use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use shrinkwraprs::Shrinkwrap;
use derive_more::Into;
use crate::error::DatabaseResult;
use crate::raw_db::RAW_DB_HANDLER;

#[derive(Clone, Debug, Shrinkwrap, Into)]
pub struct DbWrite<Kind: DbKindT>(DbRead<Kind>);

impl<Kind: DbKindT + Send + Sync + 'static> DbWrite<Kind> {
    pub fn open(
        path_prefix: &Path,
        kind: Kind,
    ) -> DatabaseResult<Self> {
        RAW_DB_HANDLER.get_or_insert(&kind, path_prefix, |kind| {
            Self::new(Some(path_prefix), kind)
        })
    }
    pub fn new(
        path_prefix: Option<&Path>,
        kind: Kind,
    ) -> DatabaseResult<Self> {
        todo!()
    }
}

pub trait DbKindT: Clone + std::fmt::Debug + Send + Sync + 'static {
    fn kind(&self) -> DbKind;

    fn filename(&self) -> PathBuf {
        let mut path = self.filename_inner();
        path.set_extension("sqlite3");
        path
    }

    fn filename_inner(&self) -> PathBuf;

    fn if_corrupt_wipe(&self) -> bool;
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, derive_more::Display)]
/// Specifies the environment used by a Conductor
pub struct DbKindZchronod;

impl DbKindT for DbKindZchronod {
    fn kind(&self) -> DbKind {
        DbKind::Zchronod
    }

    fn filename_inner(&self) -> PathBuf {
        ["zchronod", "zchronod"].iter().collect()
    }

    fn if_corrupt_wipe(&self) -> bool {
        false
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Hash, derive_more::Display)]
pub enum DbKind {
    //todo add db kind
    Zchronod,
}

#[derive(Clone, Debug)]
pub struct DbRead<Kind: DbKindT> {
    kind: Kind,
    path: PathBuf,
}