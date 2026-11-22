//! Accept only acknowledged, unconditional literal plan-only code-mode cells.
//! Never evaluate JavaScript or infer plans from branches, variables or comments.
use serde_json::Value;

pub(super) fn literal_plan(input:&str)->Option<Value>{
    let mut rest=input.trim();let mut last=None;let mut count=0;
    while !rest.is_empty(){
        rest=rest.strip_prefix("await tools.update_plan(")?;
        let mut json=String::new();let mut chars=rest.char_indices().peekable();let mut end=None;let mut depth=0i32;
        while let Some((i,ch))=chars.next(){
            match ch{
                '"'=>{json.push(ch);let mut closed=false;while let Some((_,c))=chars.next(){json.push(c);if c=='\\'{json.push(chars.next()?.1)}else if c=='"'{closed=true;break}}if !closed{return None}},
                '{'|'['=>{depth+=1;json.push(ch)},
                '}'|']'=>{depth-=1;if depth<0{return None}json.push(ch)},
                ')' if depth==0=>{end=Some(i+1);break},
                ':'|','=>json.push(ch),
                c if c.is_whitespace()=>json.push(c),
                c if c.is_ascii_alphabetic()||c=='_'=>{
                    let mut word=String::from(c);while chars.peek().is_some_and(|(_,c)|c.is_ascii_alphanumeric()||*c=='_'){word.push(chars.next()?.1)}
                    if !chars.clone().find(|(_,c)|!c.is_whitespace()).is_some_and(|(_,c)|c==':'){return None}
                    json.push('"');json.push_str(&word);json.push('"');
                },
                _=>return None,
            }
        }
        let args:Value=serde_json::from_str(&json).ok()?;
        // Require the real tool argument schema, not arbitrary source literals.
        super::steps(&args["plan"]).ok()?;
        if args.as_object()?.keys().any(|key|!matches!(key.as_str(),"plan"|"explanation")){return None}
        if args.get("explanation").is_some_and(|v|!v.is_string()){return None}
        count+=1;if count>200{return None}last=Some(args);
        rest=rest.get(end?..)?.trim_start().strip_prefix(';')?.trim_start();
    }
    last
}
