use md5::{Digest, Md5};

use super::{Account, AccountKind};

pub fn is_valid_name(name: &str) -> bool {
    (3..=16).contains(&name.len()) && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

pub fn offline_uuid(name: &str) -> uuid::Uuid {
    let mut bytes: [u8; 16] = Md5::digest(format!("OfflinePlayer:{name}")).into();
    bytes[6] = (bytes[6] & 0x0f) | 0x30;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    uuid::Uuid::from_bytes(bytes)
}

pub fn new_account(name: &str) -> Option<Account> {
    is_valid_name(name).then(|| Account {
        id: offline_uuid(name).hyphenated().to_string(),
        name: name.to_string(),
        kind: AccountKind::Offline,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offline_uuid_matches_vanilla() {
        assert_eq!(
            offline_uuid("Notch").to_string(),
            "b50ad385-829d-3141-a216-7e7d7539ba7f"
        );
        assert_eq!(
            offline_uuid("InfernumTest").to_string(),
            "c9e3c8b0-b1a6-3d94-bb44-30655e40c01a"
        );
        assert_ne!(offline_uuid("notch"), offline_uuid("Notch"));
    }

    #[test]
    fn name_rules() {
        assert!(is_valid_name("Abc"));
        assert!(is_valid_name("Player_123456789"));
        assert!(!is_valid_name("ab"));
        assert!(!is_valid_name("Player_1234567890"));
        assert!(!is_valid_name("bad-name"));
        assert!(!is_valid_name("ñandú"));
    }
}
