import {spawnSync} from 'node:child_process';

export const Velella = async ({directory}={}) => {
  const roots=new Set(process.env.VELELLA_RESTORE_NATIVE_ID?[process.env.VELELLA_RESTORE_NATIVE_ID]:[]),children=new Set();
  let active=process.env.VELELLA_RESTORE_NATIVE_ID;
  const emit = (name, id) => {
    if (!id) throw new Error('Velella lifecycle session ID is missing');
    const child = spawnSync(process.env.VELELLA_RUNTIME_EXE, ['orchestration-hook', name], {
      input: JSON.stringify({session_id: id,cwd:directory||process.cwd()}), env: process.env, windowsHide: true,
      encoding: 'utf8', timeout: 10000, maxBuffer: 1024 * 1024,
    });
    if (child.status !== 0) throw new Error('Velella lifecycle receipt failed');
  };
  return {
    'chat.message': async (input) => { if(children.has(input.sessionID))return;if(!roots.has(input.sessionID))throw new Error('Velella role root session is not bound');active=input.sessionID;emit('UserPromptSubmit', input.sessionID); },
    event: async ({event}) => {
      const id=event.properties?.sessionID||event.properties?.info?.id;
      if(event.type==='session.created'){if(event.properties?.info?.parentID){children.add(id);return;}roots.add(id);active=id;}
      const names = {'session.created': 'SessionStart', 'session.idle': 'Stop', 'session.deleted': 'Interrupt', 'permission.asked': 'PermissionRequest'};
      if (names[event.type]&&id===active&&roots.has(id)&&!children.has(id)) emit(names[event.type], id);
    },
  };
};
