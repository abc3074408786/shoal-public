import {invoke} from './desktop-platform';
import {currentTarget} from './execution-target';

export type CheckStatus='success'|'pending'|'failure'|'cancelled'|'skipped'|'unknown';
export interface PullCheck {id:string;name:string;status:CheckStatus;label:string;url:string|null;workflow:string;started_at:string;completed_at:string}
export interface PullChecks {number:number;title:string;url:string;state:string;draft:boolean;head_sha:string;head_branch:string;base_branch:string;updated_at:string;review_decision:string;checks:PullCheck[];summary:Record<CheckStatus,number>}
export interface PrChecksState {state:string;message:string;checked_at:string;branch?:string;repository?:{name:string;url:string};pull_requests?:PullChecks[]}
export async function readPrChecks(workspaceId:string):Promise<PrChecksState>{
  if(!window.velellaDesktop)return {state:'desktop_required',message:'请在鱼群桌面应用中查看 PR 检查。',checked_at:new Date().toISOString()};
  // Read the host before the await so a request in flight keeps the host it was issued
  // against; the main process repeats the check before touching any local repository path.
  const target=currentTarget();
  if(target.kind!=='local')return {state:'remote_unsupported',message:'评审检查读取的是本机仓库的 Git 与 GitHub 状态。远端执行主机的检查尚未接入目标运行时，这里不会用本机结果代替远端。',checked_at:new Date().toISOString()};
  return invoke('pr_checks',{workspace_id:workspaceId,target});
}
