import {Fragment,ReactNode,useId,useMemo,useState,useEffect} from 'react';
import {marked,type Token,type Tokens} from 'marked';
import {Check,Copy,List,Image as ImageIcon,ChevronDown} from 'lucide-react';
import {Button} from './ui';

// Reading-only recognition for common pasted notes. The stored source is never changed.
export function readingSource(source:string){
  const lines=source.replace(/\r\n/g,'\n').split('\n'),out:string[]=[];
  let fence:{marker:string;length:number}|null=null;
  const startsPython=(line:string)=>/^(?:import\s+[\w.]+(?:\s+as\s+\w+)?\s*$|from\s+[\w.]+\s+import\s+|class\s+\w.*:\s*$|def\s+\w.*:\s*$)/.test(line);
  const continuesPython=(line:string)=>!line.trim()||/^\s+\S|^(?:import |from |class |def |@|#\s|if |elif |else:|for |while |try:|except |finally:|with |return |raise |pass$)|^[A-Za-z_]\w*(?:\.\w+)*\s*(?:=|\()/.test(line);
  for(let i=0;i<lines.length;i++){
    const line=lines[i],match=line.match(/^\s{0,3}(`{3,}|~{3,})/);
    if(match){out.push(line);if(!fence)fence={marker:match[1][0],length:match[1].length};else if(match[1][0]===fence.marker&&match[1].length>=fence.length&&/^\s*$/.test(line.slice(line.indexOf(match[1])+match[1].length)))fence=null;continue;}
    if(fence){out.push(line);continue;}
    if(/^[一二三四五六七八九十百]+、\s*[^。！？]{2,40}$/.test(line.trim())){out.push('',`## ${line.trim()}`,'');continue;}
    if(startsPython(line)){
      const code=[line];while(i+1<lines.length&&continuesPython(lines[i+1])&&!/^\s{0,3}(?:`{3,}|~{3,})/.test(lines[i+1]))code.push(lines[++i]);
      while(code.at(-1)==='')code.pop();out.push('','```python',...code,'```','');continue;
    }
    out.push(line);
  }
  return out.join('\n');
}

const decode=(text:string)=>text.replace(/&(?:amp|lt|gt|quot|apos|#39|#\d+|#x[\da-f]+);/gi,entity=>{
  const named:Record<string,string>={'&amp;':'&','&lt;':'<','&gt;':'>','&quot;':'"','&apos;':"'",'&#39;':"'"};
  if(named[entity.toLowerCase()])return named[entity.toLowerCase()];
  const n=entity[2].toLowerCase()==='x'?parseInt(entity.slice(3),16):parseInt(entity.slice(2),10);
  return Number.isFinite(n)&&n>0&&n<=0x10ffff?String.fromCodePoint(n):entity;
});
function safeHref(value:string){
  if(/^#[A-Za-z0-9_\-:.]+$/.test(value))return value;
  try{const url=new URL(value);return /^https?:$/.test(url.protocol)&&!url.username&&!url.password?url.href:null}catch{return null}
}
function inline(tokens:Token[]|undefined):ReactNode{return tokens?.map((token,index)=>{
  switch(token.type){
    case 'strong':return <strong key={index}>{inline((token as Tokens.Strong).tokens)}</strong>;
    case 'em':return <em key={index}>{inline((token as Tokens.Em).tokens)}</em>;
    case 'del':return <del key={index}>{inline((token as Tokens.Del).tokens)}</del>;
    case 'codespan':return <code key={index}>{decode((token as Tokens.Codespan).text)}</code>;
    case 'br':return <br key={index}/>;
    case 'link':{const link=token as Tokens.Link,href=safeHref(link.href);return href?<a key={index} href={href} target={href.startsWith('#')?undefined:'_blank'} rel="noopener noreferrer">{inline(link.tokens)}</a>:<Fragment key={index}>{inline(link.tokens)}</Fragment>;}
    case 'image':return <span className="document-image-note" key={index}><ImageIcon size={15}/>{decode((token as Tokens.Image).text)||'图片'}</span>;
    case 'html':return <Fragment key={index}>{(token as Tokens.HTML).text}</Fragment>;
    default:{const text=token as Tokens.Text;return <Fragment key={index}>{text.tokens?inline(text.tokens):decode(text.text||token.raw||'')}</Fragment>;}
  }
})}
function plain(tokens:Token[]|undefined):string{return tokens?.map(t=>t.type==='image'?(t as Tokens.Image).text:'tokens' in t&&Array.isArray(t.tokens)?plain(t.tokens):'text' in t?String(t.text):'').join('')||''}

function CodeBlock({token}:{token:Tokens.Code}){
  const [status,setStatus]=useState('');
  useEffect(()=>{if(!status)return;const timer=setTimeout(()=>setStatus(''),2000);return()=>clearTimeout(timer)},[status]);
  return <figure className="document-code"><figcaption><span>{token.lang?.split(/\s/)[0]||'代码'}</span><Button aria-label="复制代码" title={status||'复制代码'} onClick={async()=>{try{await navigator.clipboard.writeText(token.text);setStatus('已复制')}catch{setStatus('复制失败，请选择代码复制')}}}>{status==='已复制'?<Check size={14}/>:<Copy size={14}/>}<span aria-live="polite">{status||'复制'}</span></Button></figcaption><pre><code>{token.text}</code></pre></figure>;
}

export default function DocumentReader({source,path}:{source:string;path:string}){
  const id=useId().replace(/:/g,'');
  const document=useMemo(()=>{
    const tokens=marked.lexer(readingSource(source),{gfm:true,breaks:false});
    const first=tokens.find(t=>t.type!=='space');let title=path.split(/[\\/]/).at(-1)?.replace(/\.(md|markdown|mdown)$/i,'')||path;
    if(first?.type==='heading'&&(first as Tokens.Heading).depth===1){title=plain((first as Tokens.Heading).tokens);tokens.splice(tokens.indexOf(first),1)}
    else if(first?.type==='paragraph'){const text=(first as Tokens.Paragraph).text;if(!text.includes('\n')&&text.length<=90&&!/[。！？]$/.test(text)&&!text.startsWith('<')){title=plain((first as Tokens.Paragraph).tokens);tokens.splice(tokens.indexOf(first),1)}}
    const headings=tokens.filter(t=>t.type==='heading').map((t,i)=>({token:t,id:`document-${id}-${i}`,title:plain((t as Tokens.Heading).tokens),depth:(t as Tokens.Heading).depth}));
    return {tokens,title,headings};
  },[source,path,id]);
  function blocks(tokens:Token[]):ReactNode{return tokens.map((token,index)=>{
    switch(token.type){
      case 'space':case 'def':return null;
      case 'heading':{const heading=token as Tokens.Heading,Tag=`h${Math.max(2,heading.depth)}` as 'h2'|'h3'|'h4'|'h5'|'h6';return <Tag id={document.headings.find(h=>h.token===token)?.id} key={index}>{inline(heading.tokens)}</Tag>;}
      case 'paragraph':return <p key={index}>{inline((token as Tokens.Paragraph).tokens)}</p>;
      case 'code':return <CodeBlock key={index} token={token as Tokens.Code}/>;
      case 'blockquote':return <blockquote key={index}>{blocks((token as Tokens.Blockquote).tokens)}</blockquote>;
      case 'hr':return <hr key={index}/>;
      case 'list':{const list=token as Tokens.List,Tag=list.ordered?'ol':'ul';return <Tag key={index} start={list.ordered?Number(list.start)||1:undefined}>{list.items.map((item,i)=><li key={i}>{item.task&&<input type="checkbox" checked={!!item.checked} disabled aria-label={item.checked?'已完成':'未完成'}/>}<div>{blocks(item.tokens)}</div></li>)}</Tag>;}
      case 'table':{const table=token as Tokens.Table;return <div className="document-table" key={index}><table><thead><tr>{table.header.map((cell,i)=><th style={{textAlign:table.align[i]||undefined}} key={i}>{inline(cell.tokens)}</th>)}</tr></thead><tbody>{table.rows.map((row,i)=><tr key={i}>{row.map((cell,j)=><td style={{textAlign:table.align[j]||undefined}} key={j}>{inline(cell.tokens)}</td>)}</tr>)}</tbody></table></div>;}
      case 'html':return <p className="document-literal" key={index}>{(token as Tokens.HTML).text}</p>;
      default:{const text=token as Tokens.Text;return <Fragment key={index}>{text.tokens?inline(text.tokens):decode(text.text||token.raw||'')}</Fragment>;}
    }
  })}
  return <div className="document-reader" tabIndex={0} aria-label="文档阅读区"><article className="document-sheet"><header className="document-title"><h1>{document.title}</h1><div className="document-meta"><span title={path}>{path}</span>{document.headings.length>0&&<details className="document-outline"><summary><List size={14}/>目录<ChevronDown size={12}/></summary><nav aria-label="文档目录">{document.headings.map(h=><a key={h.id} href={`#${h.id}`} onClick={e=>{e.preventDefault();e.currentTarget.closest('.document-reader')?.querySelector(`[id="${h.id}"]`)?.scrollIntoView({block:'start'});e.currentTarget.closest('details')?.removeAttribute('open')}}>{h.title}</a>)}</nav></details>}</div></header><div className="document-prose">{blocks(document.tokens)}</div></article></div>;
}
