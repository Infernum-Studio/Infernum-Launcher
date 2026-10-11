pub mod microsoft;
pub mod offline;
pub mod skins;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AccountKind {
    Offline,
    Microsoft,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub id: String,
    pub name: String,
    pub kind: AccountKind,
}

impl Account {
    pub fn is_valid(&self) -> bool {
        uuid::Uuid::parse_str(&self.id).is_ok()
            && match self.kind {
                AccountKind::Offline => offline::is_valid_name(&self.name),
                AccountKind::Microsoft => !self.name.is_empty(),
            }
    }

    pub fn offline_credentials(&self) -> theseus::prelude::Credentials {
        let id = uuid::Uuid::parse_str(&self.id).unwrap_or_default();
        theseus::prelude::Credentials::offline(self.name.clone(), id)
    }
}
