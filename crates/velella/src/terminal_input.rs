// Protocol replies are a closed, bounded grammar, never arbitrary shell text.
pub fn is_pointer_report(text:&str)->bool {
    if text.len()>64{return false;}
    let Some(body)=text.strip_prefix("\x1b[<").and_then(|s|s.strip_suffix('M').or_else(||s.strip_suffix('m'))) else{return false};
    let values=body.split(';').map(str::parse::<u16>).collect::<Result<Vec<_>,_>>();
    values.is_ok_and(|v|v.len()==3&&v[0]<=2047&&v[1]>0&&v[2]>0)
}

pub fn is_protocol_reply(text: &str) -> bool {
    if text.len()>512 || !text.is_ascii() { return false; }
    if ["\x1b[I","\x1b[O"].contains(&text) { return true; }
    let numeric=|s:&str| !s.is_empty() && s.bytes().all(|b|b.is_ascii_digit()||b==b';');
    if let Some(csi)=text.strip_prefix("\x1b[") {
        if let Some(flags)=csi.strip_prefix('?').and_then(|s|s.strip_suffix('u')) {
            return !flags.is_empty() && flags.bytes().all(|b|b.is_ascii_digit()) && flags.parse::<u8>().is_ok_and(|n|n<=31);
        }
        if csi=="0n" {return true;}
        if let Some(body)=csi.strip_suffix("$y") {return numeric(body.trim_start_matches('?'));}
        if let Some(body)=csi.strip_suffix('c') {return (body.starts_with('?')||body.starts_with('>'))&&numeric(&body[1..]);}
        if let Some(body)=csi.strip_suffix('R') {return numeric(body.trim_start_matches('?'))&&body.contains(';');}
        if let Some(body)=csi.strip_suffix('t') {return numeric(body)&&["4;","6;","8;"].iter().any(|p|body.starts_with(p));}
    }
    if let Some(osc)=text.strip_prefix("\x1b]") {
        let Some(body)=osc.strip_suffix("\x1b\\").or_else(||osc.strip_suffix('\x07')) else{return false};
        let Some((head,color))=body.split_once("rgb:") else{return false};
        let valid_head=["10;","11;","12;"].contains(&head) || head.strip_prefix("4;").and_then(|s|s.strip_suffix(';')).is_some_and(|s|!s.is_empty()&&s.bytes().all(|b|b.is_ascii_digit()));
        return valid_head && color.split('/').count()==3 && color.split('/').all(|s|!s.is_empty()&&s.len()<=4&&s.bytes().all(|b|b.is_ascii_hexdigit()));
    }
    false
}

#[cfg(test)] mod tests {
    use super::*;
    #[test] fn accepts_only_terminal_replies(){
        for s in ["\x1b[I","\x1b[O","\x1b[?1;2c","\x1b[>0;276;0c","\x1b[1;1R","\x1b[0n","\x1b[?2026;1$y","\x1b[?0u","\x1b[?31u","\x1b]11;rgb:0000/0000/0000\x1b\\"] {assert!(is_protocol_reply(s),"{s:?}");}
        for s in ["ls\r","，，有","\x03","\x1b[A","\x1b[?32u","\x1b[?1;2u","\x1b[?1uwhoami\r","\x1b]11;rgb:0000/0000/0000\x1b\\whoami\r","\x1b[?1;2c\r",""] {assert!(!is_protocol_reply(s),"{s:?}");}
    }
    #[test] fn validates_pointer_reports(){
        assert!(is_pointer_report("\x1b[<35;37;26M"));assert!(is_pointer_report("\x1b[<0;37;26m"));
        for s in ["\x1b[<0;0;1M","\x1b[<0;1;1Mwhoami\r","\x1b[<0;1M","\x1b[<9999;1;1M"]{assert!(!is_pointer_report(s))}
    }
}
