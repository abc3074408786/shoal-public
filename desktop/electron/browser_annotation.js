// Executed only in a named CDP isolated world. No page-to-workbench IPC bridge.
(() => {
  if (globalThis.__velellaAnnotation) return;
  const properties = ['font-size','font-family','color','background-color','padding','margin','border-radius'];
  let selected=null, baseline=null, fingerprint='', patch={}, overlay=null, highlight=null, label=null, picking=false, notice='';
  const originals=new Map(); let textOriginal=null, textApplied=null, regionMembers=[];
  const clipped=(value,max=500)=>String(value??'').slice(0,max);
  const identity=el=>JSON.stringify([el.textContent,el.childElementCount,...[...el.attributes].filter(a=>a.name!=='style').map(a=>[a.name,a.value])]);
  function unchanged(){
    if(baseline?.kind==='region')return baseline.url===location.href && regionMembers.every(([el,id])=>el.isConnected&&identity(el)===id);
    if(!selected?.isConnected || !baseline || baseline.url!==location.href)return false;
    // Compare the original content while allowing our own text-only preview.
    const expected=textApplied===null?selected.textContent:textOriginal;
    const current=JSON.parse(identity(selected));current[0]=expected;
    return JSON.stringify(current)===fingerprint && (textApplied===null||selected.textContent===textApplied);
  }
  function selector(el) {
    const parts=[];
    for(let e=el;e && e.nodeType===1 && parts.length<12;e=e.parentElement) {
      if(e.id && e.id.length<240 && document.querySelectorAll('#'+CSS.escape(e.id)).length===1){parts.unshift('#'+CSS.escape(e.id));break;}
      let part=e.localName;
      if(e.parentElement){const siblings=[...e.parentElement.children].filter(n=>n.localName===e.localName);if(siblings.length>1)part+=`:nth-of-type(${siblings.indexOf(e)+1})`;}
      parts.unshift(part);
    }
    return parts.join(' > ');
  }
  function restore() {
    let conflicts=false;
    if(selected?.isConnected) {
      for(const [name,original] of originals) {
        if(selected.style.getPropertyValue(name)===original.applied && selected.style.getPropertyPriority(name)==='important') {
          if(original.value) selected.style.setProperty(name,original.value,original.priority);else selected.style.removeProperty(name);
        } else conflicts=true;
      }
      if(textApplied!==null) {if(selected.childElementCount===0 && selected.textContent===textApplied)selected.textContent=textOriginal;else conflicts=true;}
    }
    originals.clear();textOriginal=textApplied=null;patch={};
    if(conflicts)notice='页面已自行更新，保留了网页的新内容；请重新选取。';
    return conflicts;
  }
  function stopPicker(){overlay?.remove();overlay=highlight=label=null;picking=false;window.removeEventListener('keydown',escape,true);}
  function clear(){restore();stopPicker();selected=baseline=null;regionMembers=[];}
  function escape(event){if(event.key==='Escape'){event.preventDefault();event.stopImmediatePropagation();clear();}}
  function valid(id) {
    if(!unchanged()){clear();throw Error('selection_stale: 页面已变化，请重新选取元素');}
    if(!id || baseline.selection_id!==id)throw Error('selection_stale: 选择已改变，请重新选取');
  }
  function status(){
    if(baseline && !unchanged()){clear();notice='页面已变化，请重新选取元素。';}
    return {ok:true,picking,selection:baseline?{...baseline,patch:{...patch}}:null,notice};
  }
  function pick(el) {
    if(!el || ['HTML','BODY','SCRIPT','STYLE','IFRAME','FRAME','OBJECT','EMBED'].includes(el.tagName)) {notice='请选取主页面中的具体元素。';return;}
    if(el.isContentEditable || el.closest('input,textarea,select')) {notice='表单输入内容不参与点选修改，请选择外部标签或按钮。';return;}
    selected=el;fingerprint=identity(el);const rect=el.getBoundingClientRect(),styles=getComputedStyle(el);
    baseline={kind:'element',selection_id:crypto.randomUUID(),url:location.href,title:clipped(document.title),selector:selector(el),tag:el.localName,
      text:clipped(el.textContent,2000),editable_text:el.childElementCount===0,
      bounds:{x:rect.x,y:rect.y,width:rect.width,height:rect.height},
      viewport:{width:innerWidth,height:innerHeight},styles:Object.fromEntries(properties.map(k=>[k,clipped(styles.getPropertyValue(k))]))};
    notice='';stopPicker();
  }
  function pickRegion(rect){
    const items=[];regionMembers=[];
    // Gather bounded leaf labels only; never read form values, editable text or frames.
    const walker=document.createTreeWalker(document.body,NodeFilter.SHOW_ELEMENT);
    let el,visited=0;
    while((el=walker.nextNode()) && ++visited<=10000 && items.length<24){
      if(el.childElementCount || el.closest('input,textarea,select,[contenteditable],iframe,frame,object,embed,script,style,[data-velella-picker]'))continue;
      const r=el.getBoundingClientRect();
      if(r.width<=0||r.height<=0||r.right<=rect.x||r.left>=rect.x+rect.width||r.bottom<=rect.y||r.top>=rect.y+rect.height||getComputedStyle(el).visibility!=='visible')continue;
      const text=clipped(el.textContent||el.getAttribute('alt'),200).trim();if(!text&&!['IMG','CANVAS','VIDEO'].includes(el.tagName))continue;
      items.push({selector:selector(el),tag:el.localName,text});regionMembers.push([el,identity(el)]);
    }
    baseline={kind:'region',selection_id:crypto.randomUUID(),url:location.href,title:clipped(document.title),selector:'',tag:'区域',text:items.map(i=>i.text||i.tag).join(' · ')||'所选页面区域',editable_text:false,styles:{},bounds:rect,document_bounds:{...rect,x:rect.x+scrollX,y:rect.y+scrollY},viewport:{width:innerWidth,height:innerHeight},scroll:{x:scrollX,y:scrollY},elements:items};
    notice='';stopPicker();
  }
  function start(mode='element'){
    clear();notice='';picking=true;
    let origin=null;
    overlay=document.createElement('div');
    overlay.setAttribute('data-velella-picker','');
    overlay.style.cssText='all:initial!important;position:fixed!important;inset:0!important;z-index:2147483647!important;cursor:crosshair!important;pointer-events:auto!important;';
    const shadow=overlay.attachShadow({mode:'closed'});highlight=document.createElement('div');label=document.createElement('div');
    highlight.style.cssText='position:fixed;pointer-events:none;border:2px solid #4c9ffe;background:#4c9ffe22;box-sizing:border-box;';
    label.style.cssText='position:fixed;pointer-events:none;background:#102540;color:#fff;padding:5px 8px;font:12px system-ui;border-radius:4px;max-width:320px;';
    shadow.append(highlight,label);document.documentElement.append(overlay);
    function beneath(event){overlay.style.setProperty('pointer-events','none','important');const e=document.elementFromPoint(event.clientX,event.clientY);overlay.style.setProperty('pointer-events','auto','important');return e;}
    const point=event=>({x:Math.max(0,Math.min(event.clientX,innerWidth)),y:Math.max(0,Math.min(event.clientY,innerHeight))});
    const region=event=>{const end=point(event);return {x:Math.min(origin.x,end.x),y:Math.min(origin.y,end.y),width:Math.abs(end.x-origin.x),height:Math.abs(end.y-origin.y)}};
    overlay.addEventListener('pointermove',event=>{const el=beneath(event);if(!el)return;const r=mode==='region'&&origin?region(event):el.getBoundingClientRect();Object.assign(highlight.style,{left:r.x+'px',top:r.y+'px',width:r.width+'px',height:r.height+'px'});label.textContent=mode==='region'?'拖选区域，Esc 取消':el.localName+' · 点击选取，Esc 取消';Object.assign(label.style,{left:Math.max(0,Math.min(r.x,innerWidth-250))+'px',top:Math.max(0,r.y-30)+'px'});});
    overlay.addEventListener('pointerdown',event=>{event.preventDefault();event.stopImmediatePropagation();if(event.button===0&&mode==='region'){origin=point(event);overlay.setPointerCapture(event.pointerId)}},true);
    overlay.addEventListener('pointerup',event=>{event.preventDefault();event.stopImmediatePropagation();if(mode==='region'&&origin){const rect=region(event);origin=null;if(rect.width>=8&&rect.height>=8)pickRegion(rect);else notice='请拖出至少 8 × 8 像素的区域。'}},true);
    overlay.addEventListener('pointercancel',()=>{origin=null});
    overlay.addEventListener('click',event=>{event.preventDefault();event.stopImmediatePropagation();if(mode!=='region')pick(beneath(event));},true);
    window.addEventListener('keydown',escape,true);
    return status();
  }
  function validatePatch(values) {
    if(!values || typeof values!=='object' || Array.isArray(values))throw Error('invalid preview');
    for(const [key,value] of Object.entries(values)){
      if(typeof value!=='string')throw Error('invalid preview value');
      if(key==='text'){if(!baseline.editable_text || value.length>2000)throw Error('请选择没有子元素的具体文字');}
      else if(['color','background-color'].includes(key)){if(!/^#[0-9a-f]{6}$/i.test(value))throw Error('颜色必须是六位十六进制色值');}
      else if(key==='font-family'){if(!['system-ui','sans-serif','serif','monospace'].includes(value))throw Error('unsupported font');}
      else if(['font-size','padding','margin','border-radius'].includes(key)){const n=Number(value.replace(/px$/,''));if(!/^\d+(\.\d+)?px$/.test(value)||n<0||n>128||(key==='font-size'&&n<8))throw Error('尺寸超出允许范围');}
      else throw Error('unsupported preview property');
    }
  }
  function preview(req){
    valid(req.selection_id);validatePatch(req.patch);
    if(baseline.kind==='region'&&Object.keys(req.patch).length)throw Error('区域批注不支持样式预览，请点选具体元素');
    if(restore()){selected=baseline=null;throw Error('selection_stale: 页面在预览期间变化，请重新选取');}
    for(const [key,value] of Object.entries(req.patch)){
      if(key==='text'){textOriginal=selected.textContent;selected.textContent=value;textApplied=value;}
      else {const original={value:selected.style.getPropertyValue(key),priority:selected.style.getPropertyPriority(key)};selected.style.setProperty(key,value,'important');original.applied=selected.style.getPropertyValue(key);originals.set(key,original);}
    }
    patch={...req.patch};return status();
  }
  globalThis.__velellaAnnotation={dispatch(req){
    if(req.action==='start')return start(req.mode);
    if(req.action==='status')return status();
    if(req.action==='cancel'){clear();return status();}
    valid(req.selection_id);
    if(req.action==='preview')return preview(req);
    if(req.action==='reset'){restore();return status();}
    if(req.action==='prepare'){
      const selection={...baseline,patch:{...patch}};
      if(restore()){selected=baseline=null;throw Error('selection_stale: 页面在预览期间变化，请重新选取');}
      return {ok:true,selection};
    }
    throw Error('unsupported annotation action');
  }};
})();
