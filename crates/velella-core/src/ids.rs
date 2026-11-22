use serde::{Deserialize, Serialize};
use std::fmt;

macro_rules! id_newtype {
    ($name:ident, $prefix:expr) => {
        /// Prefixed UUIDv7 identifier (time-sortable, e.g. `tsk_018f…`).
        #[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
        pub struct $name(pub String);

        impl $name {
            pub fn generate() -> Self {
                Self(format!("{}{}", $prefix, uuid::Uuid::now_v7()))
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

id_newtype!(TaskId, "tsk_");
id_newtype!(SessionId, "ses_");
