// The persistent PTY owns console queries even when no viewer is attached.
pub(crate) const PRIMARY_DA_REPLY: &[u8] = b"\x1b[?1;2c";
const CURSOR_REPLY: &[u8] = b"\x1b[1;1R";

pub(crate) struct TerminalQueries {
    state: u8,
    csi: Vec<u8>,
    answer_primary: bool,
}

impl TerminalQueries {
    pub(crate) fn new(answer_primary: bool) -> Self {
        Self { state: 0, csi: Vec::new(), answer_primary }
    }

    pub(crate) fn replies(&mut self, bytes: &[u8]) -> Vec<u8> {
        let mut replies = Vec::new();
        for &byte in bytes {
            match self.state {
                0 => if byte == 0x1b { self.state = 1; },
                1 => match byte {
                    b'[' => { self.csi.clear(); self.state = 2; }
                    b']' | b'P' | b'^' | b'_' => self.state = 3,
                    0x1b => {}
                    _ => self.state = 0,
                },
                2 => {
                    if (0x40..=0x7e).contains(&byte) {
                        if byte == b'n' && self.csi == b"6" { replies.extend_from_slice(CURSOR_REPLY); }
                        if self.answer_primary && byte == b'c' && (self.csi.is_empty() || self.csi == b"0") {
                            // VT100 with advanced video, matching xterm's primary DA.
                            replies.extend_from_slice(PRIMARY_DA_REPLY);
                        }
                        self.state = 0;
                    } else if (0x20..=0x3f).contains(&byte) && self.csi.len() < 16 {
                        self.csi.push(byte);
                    } else { self.state = if byte == 0x1b { 1 } else { 0 }; }
                }
                3 => match byte { 0x07 => self.state = 0, 0x1b => self.state = 4, _ => {} },
                4 => self.state = if byte == b'\\' { 0 } else if byte == 0x1b { 4 } else { 3 },
                _ => unreachable!(),
            }
        }
        replies
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conpty_startup_queries_are_answered_at_every_pipe_split() {
        let stream = b"\x1b[1t\x1b[6n\x1b[c\x1b[?1004h\x1b[?9001h";
        let expected = [CURSOR_REPLY, PRIMARY_DA_REPLY].concat();
        for split in 0..=stream.len() {
            let mut parser = TerminalQueries::new(true);
            let mut actual = parser.replies(&stream[..split]);
            actual.extend(parser.replies(&stream[split..]));
            assert_eq!(actual, expected, "pipe split at {split}");
        }
    }

    #[test]
    fn primary_da_zero_is_answered_without_advertising_other_protocols() {
        let mut parser = TerminalQueries::new(true);
        assert_eq!(parser.replies(b"\x1b[0c"), PRIMARY_DA_REPLY);
        assert!(parser.replies(b"\x1b[>0c\x1b[?u\x1b[5n\x1b[61n\x1b[1c").is_empty());
        let mut legacy = TerminalQueries::new(false);
        assert_eq!(legacy.replies(b"\x1b[c\x1b[6n"), CURSOR_REPLY);
    }

    #[test]
    fn replies_never_replay_and_string_payloads_are_ignored() {
        let mut parser = TerminalQueries::new(true);
        for byte in b"\x1b]0;title \x1b[c\x07\x1bPpayload \x1b[6n\x1b\\" {
            assert!(parser.replies(&[*byte]).is_empty());
        }
        assert_eq!(parser.replies(b"\x1b[c"), PRIMARY_DA_REPLY);
        assert!(parser.replies(b"ordinary text").is_empty());
    }
}
