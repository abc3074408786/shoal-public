use serde::{Deserialize, Serialize};

/// Runtime configuration, read from `.velella/config.toml`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// TCP address the NDJSON API binds in `velella serve`.
    #[serde(default = "default_listen")]
    pub listen: String,
    /// HTTP API address (curl / phone shortcuts / webhooks).
    #[serde(default = "default_http_listen")]
    pub http_listen: String,
    /// When set, every HTTP request must carry `Authorization: Bearer <token>`
    /// (or `?token=`). Set this before exposing the HTTP API beyond localhost.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub http_token: Option<String>,
    /// How many tasks the runner executes at once (each in its own worktree).
    #[serde(default = "default_max_parallel")]
    pub max_parallel: usize,
    /// External command fired on notable state changes. Receives
    /// VELELLA_KIND / VELELLA_SUBJECT / VELELLA_STATE in its environment —
    /// wire a toast, a Slack curl, or anything else.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notify_cmd: Option<String>,
}

fn default_listen() -> String {
    "127.0.0.1:7301".into()
}

fn default_max_parallel() -> usize {
    3
}

fn default_http_listen() -> String {
    "127.0.0.1:7302".into()
}

impl Default for Config {
    fn default() -> Self {
        Self {
            listen: default_listen(),
            http_listen: default_http_listen(),
            http_token: None,
            max_parallel: default_max_parallel(),
            notify_cmd: None,
        }
    }
}

impl Config {
    pub fn parse(toml: &str) -> Result<Self, crate::CoreError> {
        Ok(toml::from_str(toml)?)
    }

    /// Content written by `velella init`; comment-only, so it parses to defaults.
    pub fn default_doc() -> &'static str {
        "# velella runtime config\n\
         # listen = \"127.0.0.1:7301\"      # NDJSON API address for `velella serve`\n\
         # http_listen = \"127.0.0.1:7302\" # HTTP API (curl / phone shortcuts / webhooks)\n\
         # http_token = \"long-secret\"     # REQUIRED before exposing HTTP beyond localhost\n\
         # max_parallel = 3               # tasks the runner executes at once\n\
         # notify_cmd = \"\"  # command fired on notable state changes; env:\n\
         #                    VELELLA_KIND, VELELLA_SUBJECT, VELELLA_STATE\n"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_and_parse() {
        let empty = Config::parse("").unwrap();
        assert_eq!(empty, Config::default());
        assert_eq!(empty.max_parallel, 3);
        assert_eq!(empty.http_listen, "127.0.0.1:7302");
        assert!(empty.http_token.is_none());
        let cfg = Config::parse(
            "listen = \"127.0.0.1:9999\"\nmax_parallel = 5\nhttp_token = \"abc\"\nnotify_cmd = \"notify-send $VELELLA_STATE\"\n",
        )
        .unwrap();
        assert_eq!(cfg.listen, "127.0.0.1:9999");
        assert_eq!(cfg.max_parallel, 5);
        assert_eq!(cfg.http_token.as_deref(), Some("abc"));
        assert!(Config::parse("bogus = 1").is_err());
    }
}
