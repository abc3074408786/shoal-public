import {Fragment,ReactNode} from 'react';

// Render common issue Markdown as React nodes; raw HTML stays literal text.
function inline(text:string):ReactNode[]{
  return text.split(/(`[^`\n]+`|\*\*[^*\n]+\*\*|\[[^\]\n]+\]\([^\s)]+\))/g).map((part,index)=>{
    if(/^`[^`\n]+`$/.test(part))return <code key={index}>{part.slice(1,-1)}</code>;
    if(/^\*\*[^*\n]+\*\*$/.test(part))return <strong key={index}>{part.slice(2,-2)}</strong>;
    const link=part.match(/^\[([^\]]+)\]\(([^\s)]+)\)$/);
    if(link)try{const url=new URL(link[2]);if(['https:','http:'].includes(url.protocol)&&!url.username&&!url.password)return <a key={index} href={url.href} target="_blank" rel="noopener noreferrer">{link[1]}</a>}catch{/* Keep unsupported links readable. */}
    return <Fragment key={index}>{part}</Fragment>;
  });
}

export default function SourceTaskBody({source}:{source:string}){
  const lines=source.replace(/\r\n?/g,'\n').split('\n'),blocks:ReactNode[]=[];
  const startsBlock=(line:string)=>/^\s*(`{3,}|~{3,})|^#{1,6}\s|^\s*([-+*]|\d+\.)\s|^>\s?/.test(line);
  for(let i=0;i<lines.length;){
    if(!lines[i].trim()){i++;continue;}
    const key=i,line=lines[i],fence=line.match(/^\s*(`{3,}|~{3,})(.*)$/);
    if(fence){
      const content:string[]=[];i++;
      const close=new RegExp(`^\\s*${fence[1][0]}{${fence[1].length},}\\s*$`);
      while(i<lines.length&&!close.test(lines[i]))content.push(lines[i++]);
      if(i<lines.length)i++;
      blocks.push(<pre key={key}><code>{content.join('\n')}</code></pre>);continue;
    }
    const heading=line.match(/^#{1,6}\s+(.+)$/);
    if(heading){blocks.push(<h4 key={key}>{inline(heading[1])}</h4>);i++;continue;}
    const list=line.match(/^\s*([-+*]|\d+\.)\s+(.*)$/);
    if(list){
      const ordered=/\d/.test(list[1]),entries:ReactNode[]=[];
      while(i<lines.length){
        const entry=lines[i].match(/^\s*([-+*]|\d+\.)\s+(.*)$/);
        if(!entry||/\d/.test(entry[1])!==ordered)break;
        const check=entry[2].match(/^\[([ xX])\]\s+(.*)$/);
        entries.push(<li key={i++}>{check?<><input type="checkbox" checked={check[1].toLowerCase()==='x'} disabled aria-label={check[2]}/>{inline(check[2])}</>:inline(entry[2])}</li>);
      }
      blocks.push(ordered?<ol key={key} start={parseInt(list[1],10)}>{entries}</ol>:<ul key={key}>{entries}</ul>);continue;
    }
    if(/^>\s?/.test(line)){
      const quote:string[]=[];while(i<lines.length&&/^>\s?/.test(lines[i]))quote.push(lines[i++].replace(/^>\s?/,''));
      blocks.push(<blockquote key={key}>{inline(quote.join('\n'))}</blockquote>);continue;
    }
    const paragraph=[lines[i++]];
    while(i<lines.length&&lines[i].trim()&&!startsBlock(lines[i]))paragraph.push(lines[i++]);
    blocks.push(<p key={key}>{inline(paragraph.join('\n'))}</p>);
  }
  return <div className="source-task-body">{blocks}</div>;
}
