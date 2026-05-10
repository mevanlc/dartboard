use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::canvas::Canvas;
use crate::color::RgbColor;
use crate::ops::CanvasOp;

pub type UserId = u64;
pub type ClientOpId = u64;
pub type Seq = u64;
pub type UserMetadata = BTreeMap<String, String>;

pub const USER_METADATA_KEY_MAX_BYTES: usize = 4 * 1024;
pub const USER_METADATA_VALUE_MAX_BYTES: usize = 64 * 1024;
pub const USER_METADATA_MAX_ENTRIES: usize = 256;
pub const USER_METADATA_TOTAL_MAX_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DartboardUser {
    pub user_id: UserId,
    pub name: String,
    pub color: RgbColor,
    #[serde(default, skip_serializing_if = "user_metadata_is_empty")]
    pub metadata: UserMetadata,
}

pub type Peer = DartboardUser;

pub fn validate_user_metadata(metadata: &UserMetadata) -> Result<(), String> {
    if metadata.len() > USER_METADATA_MAX_ENTRIES {
        return Err(format!(
            "user metadata has {} entries; max is {}",
            metadata.len(),
            USER_METADATA_MAX_ENTRIES
        ));
    }

    let mut total = 0usize;
    for (key, value) in metadata {
        if key.is_empty() {
            return Err("user metadata keys must not be empty".to_string());
        }
        if key.len() > USER_METADATA_KEY_MAX_BYTES {
            return Err(format!(
                "user metadata key {:?} is {} bytes; max is {}",
                truncate_for_message(key),
                key.len(),
                USER_METADATA_KEY_MAX_BYTES
            ));
        }
        if value.len() > USER_METADATA_VALUE_MAX_BYTES {
            return Err(format!(
                "user metadata value for key {:?} is {} bytes; max is {}",
                truncate_for_message(key),
                value.len(),
                USER_METADATA_VALUE_MAX_BYTES
            ));
        }
        total = total.saturating_add(key.len()).saturating_add(value.len());
    }

    if total > USER_METADATA_TOTAL_MAX_BYTES {
        return Err(format!(
            "user metadata is {} bytes total; max is {}",
            total, USER_METADATA_TOTAL_MAX_BYTES
        ));
    }

    Ok(())
}

fn user_metadata_is_empty(metadata: &UserMetadata) -> bool {
    metadata.is_empty()
}

fn truncate_for_message(value: &str) -> String {
    const MAX_CHARS: usize = 32;
    let mut chars = value.chars();
    let truncated: String = chars.by_ref().take(MAX_CHARS).collect();
    if chars.next().is_some() {
        format!("{truncated}...")
    } else {
        truncated
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClientMsg {
    Hello {
        name: String,
        color: RgbColor,
        #[serde(default, skip_serializing_if = "user_metadata_is_empty")]
        metadata: UserMetadata,
    },
    Op {
        client_op_id: ClientOpId,
        op: CanvasOp,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ServerMsg {
    Welcome {
        your_user_id: UserId,
        your_color: RgbColor,
        peers: Vec<DartboardUser>,
        snapshot: Canvas,
    },
    ConnectRejected {
        reason: String,
    },
    Ack {
        client_op_id: ClientOpId,
        seq: Seq,
    },
    OpBroadcast {
        from: UserId,
        op: CanvasOp,
        seq: Seq,
    },
    PeerJoined {
        peer: DartboardUser,
    },
    PeerLeft {
        user_id: UserId,
    },
    Reject {
        client_op_id: ClientOpId,
        reason: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metadata_with_entries(count: usize) -> UserMetadata {
        (0..count)
            .map(|i| (format!("key.{i}"), format!("value.{i}")))
            .collect()
    }

    #[test]
    fn user_metadata_allows_configured_entry_limit() {
        let metadata = metadata_with_entries(USER_METADATA_MAX_ENTRIES);
        assert_eq!(validate_user_metadata(&metadata), Ok(()));
    }

    #[test]
    fn user_metadata_rejects_too_many_entries() {
        let metadata = metadata_with_entries(USER_METADATA_MAX_ENTRIES + 1);
        assert!(validate_user_metadata(&metadata)
            .unwrap_err()
            .contains("entries"));
    }

    #[test]
    fn user_metadata_rejects_oversized_value() {
        let mut metadata = UserMetadata::new();
        metadata.insert(
            "external.id".to_string(),
            "x".repeat(USER_METADATA_VALUE_MAX_BYTES + 1),
        );
        assert!(validate_user_metadata(&metadata)
            .unwrap_err()
            .contains("value"));
    }

    #[test]
    fn empty_metadata_is_optional_on_wire() {
        let user = DartboardUser {
            user_id: 1,
            name: "alice".to_string(),
            color: RgbColor::new(1, 2, 3),
            metadata: UserMetadata::new(),
        };

        let json = serde_json::to_string(&user).unwrap();
        assert!(!json.contains("metadata"));

        let decoded: DartboardUser =
            serde_json::from_str(r#"{"user_id":1,"name":"alice","color":{"r":1,"g":2,"b":3}}"#)
                .unwrap();
        assert!(decoded.metadata.is_empty());
    }
}
