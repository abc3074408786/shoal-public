export type PlanNode={id:string;title:string;instruction:string;role:'lead'|'scout';depends_on:string[];scope:string;checks:string[]};
export type OrchestrationPlan={acceptance:string;parallelism:number;nodes:PlanNode[]};
export type Work={id:string;work_key:string;title:string;instruction:string;state:string;assignee:string;attempt_id:string;attempt_count:number;candidate?:{revision?:string};write:boolean};
export type Member={id:string;name:string;role:string;state:string;identity:{session_id:string;workspace_id?:string}};
export type Review={id:string;work_id:string;attempt_id:string;state:string;summary?:string;reviewer:string};
export type Run={id:string;goal:string;workspace_id:string;state:string;blocker?:string;agent:string;scout_agent:string;lead_session?:string;revision:number;plan?:OrchestrationPlan;nodes?:PlanNode[];final_candidate?:{revision?:string};events:{at:string;kind:string;text:string}[];status:{members:Member[];work:Work[];reviews:Review[];attempts:{id:string;report?:{summary:string;outcome:string}}[]}};
export const runStates:Record<string,string>={planning:'正在起草',draft:'待确认方案',running:'执行中',paused:'已暂停派发',stopping:'正在停止',stopped:'已停止',awaiting_acceptance:'待你验收',accepted:'已验收'};
export const workStates:Record<string,string>={queued:'等待依赖',assigned:'已派发',input_accepted:'输入已送达',turn_started:'执行中',reported:'待独立审查',accepted:'技术通过',failed:'需要处理',cancelled:'已停止'};
export function graphPositions(nodes:PlanNode[]){
  const levels=new Map<string,number>();
  for(let pass=0;pass<nodes.length;pass++)for(const node of nodes)if(node.depends_on.every(id=>levels.has(id)))levels.set(node.id,node.depends_on.length?1+Math.max(...node.depends_on.map(id=>levels.get(id)!)):0);
  const rows=new Map<number,number>();
  return nodes.map(node=>{const level=levels.get(node.id)??0,row=rows.get(level)||0;rows.set(level,row+1);return {...node,x:24+level*240,y:24+row*100}});
}
