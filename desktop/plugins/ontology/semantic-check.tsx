import {useEffect,useState} from 'react';
import {ArrowLeft,RefreshCw} from 'lucide-react';
import {Button} from '../../src/ui';
import type {ExtensionRequest,OntologyDefinitions,SemanticFact,SemanticReport} from '../../src/extensions/types';

type Props={project:string;request:ExtensionRequest;definitions?:OntologyDefinitions;onBack:()=>void;onOpenEntity:(row:SemanticFact)=>void};
const name=(row:SemanticFact)=>row.fields.name||row.fields.title||row.id;

export default function SemanticCheckPanel({project,request,definitions,onBack,onOpenEntity}:Props){
  const [report,setReport]=useState<SemanticReport>(),[error,setError]=useState(''),[loading,setLoading]=useState(true),[run,setRun]=useState(0);
  useEffect(()=>{
    let live=true;setReport(undefined);setError('');setLoading(true);
    void request<SemanticReport>('check',{project}).then(value=>{
      if(value.project!==project||value.basis!=='recorded_facts_only'||typeof value.valid!=='boolean'||!Array.isArray(value.findings)||!Array.isArray(value.rules))throw Error('检查返回的项目或报告格式不符，请重新检查。');
      if(live)setReport(value);
    }).catch(error=>{if(live)setError(String(error))}).finally(()=>{if(live)setLoading(false)});
    return()=>{live=false};
  },[project,request,run]);
  return <section className="ontology-check" aria-label="项目语义检查" aria-busy={loading}>
    <header className="ontology-check-heading"><div><h2>语义检查</h2><p>当前项目：{project} · 只读检查</p></div><div><Button onClick={onBack}><ArrowLeft size={15}/>返回本体</Button><Button disabled={loading} onClick={()=>setRun(value=>value+1)}><RefreshCw size={15}/>{loading?'正在检查…':'重新检查'}</Button></div></header>
    <p className="ontology-check-scope">检查已登记的关系、依赖和交付证据。未发现错误仅表示本版规则内无错误，不代表真实测试、部署或业务验收通过。</p>
    {loading&&<p role="status">正在读取当前项目的事实与版本…</p>}
    {error&&<p role="alert">检查未完成：{error} 请重新检查。</p>}
    {report&&<>
      <div className="ontology-check-result" role="status"><h3>{report.summary.errors?`发现 ${report.summary.errors} 项错误`:'未发现规则内错误'}</h3><p>{report.summary.errors} 项错误 · {report.summary.warnings} 项提醒 · {report.rules.length} 条规则</p></div>
      {!report.findings.length&&<p className="ontology-check-clear">当前记录未触发检查问题。数据变化后请重新检查。</p>}
      <div className="ontology-check-findings">{report.findings.map((item,index)=>{
        const rule=report.rules.find(rule=>rule.id===item.rule_id);
        const subject='fields' in item.subject?name(item.subject):item.subject.id;
        return <details className="ontology-check-finding" key={item.rule_id+':'+item.subject.id+':'+index}>
          <summary><span className="ontology-check-severity">{item.severity==='error'?'错误':'提醒'}</span><span><strong>{rule?.title||item.rule_id} · {subject}</strong><span>{item.message}</span></span></summary>
          <div className="ontology-check-explanation"><h4>判定依据</h4><p>{rule?.statement||item.rule_id}</p><p className="ontology-check-suggestion">处理建议：{item.suggestion}</p>
            <h4>相关事实</h4>{item.facts.map((row,index)=><section className="ontology-check-fact" key={row.id+':'+index}><header><strong>{name(row)}</strong><span>{row.state} · 版本 {row.revision}</span>{row.project===project&&<Button onClick={()=>onOpenEntity(row)}>查看实体</Button>}</header><dl><div><dt>实体ID</dt><dd>{row.id}</dd></div><div><dt>来源</dt><dd>{row.source||'未记录'}</dd></div>{Object.entries(row.fields).map(([key,value])=><div key={key}><dt>{definitions?.entity_types.find(type=>type.name===row.type)?.fields.find(field=>field.name===key)?.description||key}</dt><dd>{value}</dd></div>)}</dl></section>)}
            {!!item.relations.length&&<><h4>关系依据</h4><ul>{item.relations.map(edge=><li key={edge.id}>{definitions?.relations.find(relation=>relation.kind===edge.kind)?.name_cn||edge.kind}：<code>{edge.from}</code> → <code>{edge.to}</code></li>)}</ul></>}
            <details className="ontology-check-raw"><summary>规则编号与完整依据</summary><p>规则：{item.rule_id} · 检查版本：{report.check_version}</p><pre>{JSON.stringify({facts:item.facts,relations:item.relations,missing:item.missing},null,2)}</pre></details>
          </div>
        </details>;
      })}</div>
    </>}
  </section>;
}
