export interface ActivityEvent {seq:number;event:string;time:string;native_id:string;agent_id:string;turn_id:string;call_id:string;tool:string;input:string;output:string;failed:boolean}
export interface ActivityItem {id:string;title:string;time:string;status:'running'|'returned'|'failed'|'waiting'|'interrupted'|'unknown';input:string;output:string;nativeId:string;agentId:string;turnId:string}
export function activityTitle(tool:string,input:string){
  let args:Record<string,unknown>={};try{args=JSON.parse(input)||{}}catch{/* Plain tool input remains available in details. */}
  const path=String(args.file_path||args.path||args.file||'');
  if(/^(Read|read_file)$/i.test(tool))return `读取文件${path?' · '+path:''}`;
  if(/^(Grep|Glob|search|list_files)$/i.test(tool))return `搜索文件${args.pattern?' · '+String(args.pattern):''}`;
  if(/^(Edit|Write|apply_patch|edit_file|write_file)$/i.test(tool))return `修改文件${path?' · '+path:''}`;
  if(/^(Bash|exec_command|shell|shell_command|run_command)$/i.test(tool)){
    if(typeof args.description==='string')return args.description;
    const command=String(args.command||args.cmd||'').replace(/\s+/g,' ').trim();
    return command?`运行命令 · ${command.slice(0,160)}${command.length>160?'…':''}`:'运行命令';
  }
  return tool?`调用工具 · ${tool}`:'执行操作';
}
export function mergeActivity(previous:ActivityItem[],events:ActivityEvent[]){
  const rows=previous.map(r=>({...r}));
  for(const e of events){
    const key=JSON.stringify([e.native_id,e.agent_id,e.call_id||e.seq]);
    if(['Stop','Interrupt','UserPromptSubmit','SessionStart','SessionEnd'].includes(e.event)){
      // Background Stop can arrive after the next turn has already started.
      const scopedTurn=['Stop','Interrupt'].includes(e.event)&&e.turn_id;
      for(const r of rows)if(r.nativeId===e.native_id&&r.agentId===e.agent_id&&(!scopedTurn||r.turnId===e.turn_id)&&['running','waiting'].includes(r.status))r.status=e.event==='Interrupt'?'interrupted':'unknown';
      continue;
    }
    if(!['PreToolUse','PostToolUse','PostToolUseFailure','PermissionRequest'].includes(e.event))continue;
    const old=rows.find(r=>r.id===key);
    const status:ActivityItem['status']=e.failed?'failed':e.event==='PermissionRequest'?'waiting':e.event==='PreToolUse'?'running':'returned';
    // A delayed start must not move a returned result back to running.
    if(old&&e.event==='PreToolUse'&&['returned','failed'].includes(old.status))continue;
    const row={id:key,title:activityTitle(e.tool,e.input),time:old?.time||e.time,status,input:e.input||old?.input||'',output:e.output||old?.output||'',nativeId:e.native_id,agentId:e.agent_id,turnId:e.turn_id||old?.turnId||''};
    if(old)Object.assign(old,row);else rows.push(row);
  }
  return rows.slice(-500);
}
