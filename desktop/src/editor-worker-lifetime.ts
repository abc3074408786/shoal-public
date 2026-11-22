type Monaco=typeof import('monaco-editor');
type Language='typescript'|'javascript';

// Monaco 0.52's TS/JS worker manager has no idle timeout. Reset its public
// worker options to the SAME value only when every TS/JS model is gone.
// This lets Monaco dispose its client and recreate it normally on demand.
export function manageTypeScriptWorkerLifetime(monaco:Monaco,idleMs=3000){
  const started=new Set<Language>();
  let timer:ReturnType<typeof setTimeout>|undefined,disposed=false,generation=0;
  const relevant=(language:string)=>language==='typescript'||language==='javascript';
  const hasModels=(excluding?:import('monaco-editor').editor.ITextModel)=>
    monaco.editor.getModels().some(model=>model!==excluding&&!model.isDisposed()&&relevant(model.getLanguageId()));
  const cancel=()=>{generation++;if(timer!==undefined)clearTimeout(timer);timer=undefined};
  const schedule=(disposing?:import('monaco-editor').editor.ITextModel)=>{
    cancel();
    if(disposed||!started.size||hasModels(disposing))return;
    timer=setTimeout(async()=>{
      timer=undefined;
      if(disposed||hasModels())return;
      const languages=[...started],reserved=generation;
      // Probe the existing worker before disposal, then recheck ownership. This
      // public lookup does not claim to drain every background RPC, build a new
      // TypeScript program, or modify documents. Retain an unresponsive worker.
      try{
        for(const language of languages){
          const factory=await (language==='typescript'?monaco.languages.typescript.getTypeScriptWorker():monaco.languages.typescript.getJavaScriptWorker());
          const worker=await factory();
          await worker.getScriptText('file:///__velella_idle_probe__.ts');
        }
      }catch{return}
      if(disposed||generation!==reserved||hasModels())return;
      for(const language of languages){
        if(hasModels())return;
        const defaults=language==='typescript'?monaco.languages.typescript.typescriptDefaults:monaco.languages.typescript.javascriptDefaults;
        started.delete(language);
        defaults.setWorkerOptions(defaults.workerOptions);
      }
    },idleMs);
  };
  const listeners=[
    monaco.editor.onDidCreateModel(()=>schedule()),
    monaco.editor.onWillDisposeModel(model=>schedule(model)),
    monaco.editor.onDidChangeModelLanguage(()=>schedule()),
    monaco.languages.typescript.typescriptDefaults.onDidChange(()=>{started.delete('typescript');schedule()}),
    monaco.languages.typescript.javascriptDefaults.onDidChange(()=>{started.delete('javascript');schedule()}),
  ];
  return {
    created(language:Language){if(disposed)return;started.add(language);schedule()},
    dispose(){disposed=true;cancel();for(const listener of listeners)listener.dispose()},
  };
}
