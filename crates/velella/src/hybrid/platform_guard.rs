//! Platform boundary for managed remote local-file tools.
pub(super) fn local_tools_allowed(managed: bool) -> bool {
    cfg!(windows) || !managed
}

#[cfg(test)]
mod tests {
    use super::local_tools_allowed;

    #[test]
    fn unmanaged_sessions_remain_available() {
        assert!(local_tools_allowed(false));
    }

    #[test]
    fn managed_sessions_require_windows_file_safety() {
        assert_eq!(local_tools_allowed(true), cfg!(windows));
    }
}
