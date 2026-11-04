/* All privileged operations use local Tauri commands; no credentials go to remote pages. */
'use strict';
const APP_VERSION='0.3.1';
const $ = id => document.getElementById(id);
const invoke = (name, args) => window.__TAURI__.core.invoke(name, args);
const state = {status:null,models:[],accounts:[],tasks:[],media:[],legacyRecords:[],settings:null,images:[],page:'studio',connections:[],submitting:false,loadingImages:false,activeTaskId:null,monitorKey:''};
const labels = {queued:'排队中',running:'生成中',succeeded:'已保存',failed:'失败',interrupted:'已中断',cancelled:'已取消',archived:'旧版已完成'};
const titles = {studio:'创作工作台',tasks:'任务与进度',library:'本机作品',accounts:'Google 账号',settings:'设置与接入'};
let noticeTimer;

function notice(message,error=false){
  $('notice').textContent=String(message);
  $('notice').className=error?'error':'';
  $('notice').setAttribute('role',error?'alert':'status');
  $('notice').setAttribute('aria-live',error?'assertive':'polite');
  $('notice').hidden=false;
  clearTimeout(noticeTimer);
  if(!error)noticeTimer=setTimeout(()=>$('notice').hidden=true,6500);
}
function node(tag,text,className){
  const element=document.createElement(tag);
  if(text!==undefined)element.textContent=text;
  if(className)element.className=className;
  return element;
}
async function action(button,callback){
  const old=button.disabled;
  button.disabled=true;
  try{return await callback()}catch(error){notice(error,true)}finally{button.disabled=old}
}
function button(text,callback,className='secondary'){
  const b=node('button',text,className);
  b.type='button';
  b.onclick=()=>action(b,callback);
  return b;
}

/* Universal reusable list component with search, dimension filter, sort, pagination & localStorage memory */
function createListView(config){
  const storageKey=`flow-desktop-list:${config.key}`;
  const defaultSort=config.sorts[0].value;
  const view={
    query:'',
    filter:'all',
    page:1,
    pageSize:20,
    sort:defaultSort,
    order:config.defaultOrder==='asc'?'asc':'desc',
    items:[],
    lastKey:'',
  };
  try{
    const saved=JSON.parse(localStorage.getItem(storageKey)||'null');
    if(saved&&typeof saved==='object'){
      const size=Number(saved.pageSize);
      if([10,20,50,100].includes(size))view.pageSize=size;
      if(config.sorts.some(s=>s.value===saved.sort))view.sort=saved.sort;
      if(saved.order==='asc'||saved.order==='desc')view.order=saved.order;
    }
  }catch{/* use defaults */}
  function persist(){
    try{
      localStorage.setItem(storageKey,JSON.stringify({
        pageSize:view.pageSize,
        sort:view.sort,
        order:view.order,
      }));
    }catch{/* ignore storage errors */}
  }
  const host=$(config.hostId);
  host.replaceChildren();
  const toolbar=node('div',undefined,'list-toolbar');
  const search=node('input');
  search.type='search';
  search.placeholder=config.searchPlaceholder||'关键字筛选…';
  search.setAttribute('aria-label',config.searchPlaceholder||'关键字筛选');
  search.oninput=()=>{view.query=search.value.trim().toLowerCase();view.page=1;render(true);};
  toolbar.append(search);

  let filterSelect=null;
  if(config.filters&&config.filters.length){
    filterSelect=node('select');
    filterSelect.setAttribute('aria-label',config.filterLabel||'维度筛选');
    for(const item of config.filters){
      const opt=node('option',item.label);
      opt.value=item.value;
      filterSelect.append(opt);
    }
    filterSelect.onchange=()=>{view.filter=filterSelect.value;view.page=1;render(true);};
    toolbar.append(filterSelect);
  }

  const sortSelect=node('select');
  sortSelect.setAttribute('aria-label','排序字段');
  for(const item of config.sorts){
    const opt=node('option',`排序：${item.label}`);
    opt.value=item.value;
    opt.selected=item.value===view.sort;
    sortSelect.append(opt);
  }
  const orderBtn=node('button',undefined,'secondary sort-order');
  orderBtn.type='button';
  function updateOrderButton(){
    const current=config.sorts.find(s=>s.value===view.sort)||config.sorts[0];
    orderBtn.textContent=`${current.label} ${view.order==='asc'?'↑ 正序':'↓ 逆序'}`;
  }
  updateOrderButton();
  sortSelect.onchange=()=>{
    view.sort=sortSelect.value;
    view.page=1;
    persist();
    updateOrderButton();
    render(true);
  };
  orderBtn.onclick=()=>{
    view.order=view.order==='asc'?'desc':'asc';
    view.page=1;
    persist();
    updateOrderButton();
    render(true);
  };
  toolbar.append(sortSelect,orderBtn);

  const body=node('div',undefined,config.bodyClass||'list-body');
  const pager=node('div',undefined,'list-pager');
  host.append(toolbar,body,pager);

  function renderPager(total,pages){
    pager.replaceChildren();
    const start=total?(view.page-1)*view.pageSize+1:0;
    const end=Math.min(total,view.page*view.pageSize);
    const info=node('span',total?`第 ${start}–${end} 条，共 ${total} 条`:'共 0 条');
    const sizes=node('select');
    sizes.setAttribute('aria-label','每页条数');
    for(const n of [10,20,50,100]){
      const opt=node('option',`${n} 条/页`);
      opt.value=String(n);
      opt.selected=n===view.pageSize;
      sizes.append(opt);
    }
    sizes.onchange=()=>{
      view.pageSize=Number(sizes.value);
      view.page=1;
      persist();
      render(true);
    };
    const nav=node('div',undefined,'list-pager-nav');
    const controls=[
      ['首页',1,view.page<=1],
      ['上一页',view.page-1,view.page<=1],
      null,
      ['下一页',view.page+1,view.page>=pages],
      ['末页',pages,view.page>=pages],
    ];
    for(const item of controls){
      if(!item){
        nav.append(node('span',`${view.page} / ${pages}`));
        continue;
      }
      const [label,targetPage,disabled]=item;
      const b=node('button',label,'secondary');
      b.type='button';
      b.disabled=disabled;
      b.onclick=()=>{view.page=targetPage;render(true);};
      nav.append(b);
    }
    pager.append(info,sizes,nav);
  }

  function render(force=false){
    const raw=view.items||[];
    const filtered=raw.filter(item=>{
      if(view.filter!=='all'&&config.matchFilter&&!config.matchFilter(item,view.filter))return false;
      if(view.query&&!config.searchText(item).toLowerCase().includes(view.query))return false;
      return true;
    });
    const sortDef=config.sorts.find(s=>s.value===view.sort)||config.sorts[0];
    const dir=view.order==='asc'?1:-1;
    filtered.sort((a,b)=>{
      const va=sortDef.get(a);
      const vb=sortDef.get(b);
      if(typeof va==='number'&&typeof vb==='number')return (va-vb)*dir;
      return String(va??'').localeCompare(String(vb??''),'zh-CN',{numeric:true})*dir;
    });
    const total=filtered.length;
    const pages=Math.max(1,Math.ceil(total/view.pageSize)||1);
    view.page=Math.min(Math.max(1,view.page),pages);
    const start=(view.page-1)*view.pageSize;
    const pageItems=filtered.slice(start,start+view.pageSize);
    const revision=config.revisionKey?JSON.stringify([view.page,view.pageSize,view.sort,view.order,view.query,view.filter,raw.length,config.revisionKey(pageItems)]):null;
    if(!force&&revision&&revision===view.lastKey)return;
    if(revision)view.lastKey=revision;
    body.replaceChildren();
    if(!raw.length){
      body.append(node('div',config.emptyText||'暂无记录。','empty'));
    }else if(!pageItems.length){
      body.append(node('div',config.noMatchText||'没有匹配筛选条件的记录。','empty'));
    }else{
      for(const item of pageItems)body.append(config.renderItem(item));
    }
    renderPager(total,pages);
  }

  return {
    update(items,force=false){
      view.items=Array.isArray(items)?items:[];
      render(force);
    },
    refresh(){render(true);},
  };
}

function page(name,{focus=true}={}){
  state.page=name;
  document.querySelectorAll('.page').forEach(element=>element.hidden=element.id!==name);
  document.querySelectorAll('[data-page]').forEach(element=>{
    const active=element.dataset.page===name;
    element.classList.toggle('active',active);
    if(active)element.setAttribute('aria-current','page');
    else element.removeAttribute('aria-current');
  });
  $('page-title').textContent=titles[name];
  document.title=titles[name]+' · Flow2API';
  if(focus)$('page-title').focus({preventScroll:true});
}
page(state.page,{focus:false});
document.querySelectorAll('[data-page]').forEach(element=>element.onclick=()=>page(element.dataset.page));
document.querySelectorAll('[data-go]').forEach(element=>element.onclick=()=>page(element.dataset.go));

function updateStatus(status){
  state.status=status;
  const running=status.service==='running';
  $('service-pill').className=status.service;
  $('service-pill').querySelector('span').textContent=running?'API 运行中':status.service==='failed'?'API 启动失败':status.service==='starting'?'API 启动中':'API 已停止';
  $('api-state').textContent=running?'运行中':status.service==='failed'?'需要处理':'已停止';
  $('api-address').textContent=status.address||'仅监听本机地址';
  $('job-count').replaceChildren(node('span',String(status.running)),document.createTextNode(' '),node('em','执行中'));
  $('queue-count').textContent=`${status.queued} 个等待执行`;
  $('nav-count').textContent=status.running+status.queued;
  $('error-count').textContent=status.failed?`${status.failed} 个失败／中断任务`:'任务状态正常';
  $('toggle-service').textContent=running?'停止 API':'启动 API';
  $('toggle-service').disabled=status.running+status.queued>0||['starting','stopping'].includes(status.service);
  $('data-path').textContent=status.data_dir;
  $('browser-status').textContent=status.browser;
  if(status.address)$('base-url').textContent=status.address+'/v1';
  updateReadiness();
  if(status.last_error&&status.service==='failed')notice(status.last_error,true);
}

function selectedModel(){return state.models.find(model=>model.id===$('model').value)}
const kindLabels={image:'文生图 / 参考图编辑',t2v:'文字生成视频',omni:'文字 / 参考图视频',i2v:'首帧 / 首尾帧视频',r2v:'多参考图视频',extend:'续写已有视频'};
const pickerIds={type:'model-type',family:'model-family',kind:'model-kind',aspect_ratio:'model-ratio',spec:'model-spec'};
function pickerLabel(field,value){
  if(field==='type')return value==='image'?'图片':'视频';
  if(field==='kind')return kindLabels[value]||value;
  if(field==='spec')return $('model-type').value==='video'?value+' 秒':value==='基础'?'基础（默认）':value;
  return value;
}
function applyPicker(wanted){
  const result=FlowStudio.choices(state.models,wanted);
  for(const [field,id]of Object.entries(pickerIds)){
    const select=$(id);
    select.replaceChildren(...result.options[field].map(value=>{
      const option=node('option',pickerLabel(field,value));option.value=value;return option;
    }));
    select.value=result.selected[field];
  }
  $('kind-field').hidden=result.options.kind.length<2;
  $('spec-label').textContent=result.selected.type==='image'?'清晰度':'时长';
  $('model').value=result.model?.id||'';
  modelInfo();
}
function modelInfo(){
  const model=selectedModel();
  if(!model)return;
  const c=model.capabilities;
  $('model-info').textContent=c.summary;
  $('video-field').hidden=!c.requires_video_id;
  const price=c.pricing;
  $('model-price').textContent=price.credits===null?'此规格费用未核价，以账号实际扣费为准':`${price.verified_at} 参考：${price.credits} credits / ${price.unit}，以实际扣费为准`;
  $('images').disabled=!c.supports_images||state.loadingImages;
  $('reference-hint').textContent=c.supports_images?`支持 ${c.min_images}–${c.max_images} 张，每张不超过 20 MB。${c.kind==='i2v'?'第 1 张为首帧，第 2 张为尾帧。':''}`:`此用途不上传参考图。${state.images.length?'已选图片暂时保留，切回图片用途可继续使用。':''}`;
  renderReferences();
}
function renderModels(){
  $('model').replaceChildren(...state.models.map(model=>{const option=node('option',model.id);option.value=model.id;return option;}));
  const initial=state.models.find(m=>m.id===draft.model)||state.models.find(m=>m.id==='gemini-3.1-flash-lite-image-square')||state.models[0];
  if(initial)applyPicker(FlowStudio.selection(initial));
}
for(const id of Object.values(pickerIds))$(id).onchange=()=>{
  const wanted=Object.fromEntries(Object.entries(pickerIds).map(([field,key])=>[field,$(key).value]));
  applyPicker(wanted);saveDraft();clearComposeError();
};

function studioRequest(){
  const model=selectedModel();
  return {model:model?.id||'',prompt:$('prompt').value.trim(),images:model?.capabilities.supports_images?[...state.images]:[],account_id:$('account').value||null,video_id:model?.capabilities.requires_video_id?$('video-id').value.trim()||null:null};
}
function clearComposeError(){
  $('compose-error').hidden=true;
  for(const id of ['prompt','images','video-id','model-family']){$(id).removeAttribute('aria-invalid');$(id).removeAttribute('aria-errormessage');}
}
function showComposeError(error){
  $('compose-error').textContent=error.message;$('compose-error').hidden=false;
  $(error.field).setAttribute('aria-invalid','true');
  $(error.field).setAttribute('aria-errormessage','compose-error');
  $(error.field).focus();
}
function updateReadiness(){
  const running=state.status?.service==='running';
  const ready=state.accounts.filter(accountReady);
  const chosen=$('account').value;
  let title='可以开始创作',detail=ready.length===1?`使用 ${ready[0].email}，作品自动保存到本机。`:`${ready.length} 个账号就绪，自动选择空闲账号。`,actionText='';
  if(!running){title=state.status?.service==='failed'?'本机 API 启动失败':'本机 API 尚未启动';detail=state.status?.last_error||'启动后即可连接浏览器并创作。';actionText='启动 API';}
  else if(!ready.length){title='先连接 Flow 账号';detail='在 Google 账号页完成连接，保留 Flow 标签页打开后即可创作。';actionText='连接账号';}
  else if(chosen&&!ready.some(a=>a.id===chosen)){title='所选账号暂不可用';detail='重新连接该账号，或切换为自动选择就绪账号。';actionText='检查账号';}
  if(state.loadingImages){title='正在读取参考图';detail='图片准备好后即可提交。';}
  $('readiness-title').textContent=title;$('readiness-detail').textContent=detail;
  $('readiness').classList.toggle('ready',running&&ready.length>0&&(!chosen||ready.some(a=>a.id===chosen)));
  $('readiness-action').hidden=!actionText;$('readiness-action').textContent=actionText;
  $('generate').disabled=state.submitting||state.loadingImages||!running||!ready.length||Boolean(chosen&&!ready.some(a=>a.id===chosen));
  $('generate').textContent=state.submitting?'正在提交…':'开始创作 ↗';
}
$('readiness-action').onclick=()=>action($('readiness-action'),async()=>{
  if(state.status?.service!=='running'){await invoke('start_service');await refresh();}
  else page('accounts');
});
$('back-to-studio').onclick=()=>page('studio');

const DRAFT_KEY='flow-desktop-draft:v1';
let draft={};
try{draft=JSON.parse(localStorage.getItem(DRAFT_KEY)||'{}')||{};}catch{/* fresh draft */}
let draftDB=null,referenceRevision=0;
function saveDraft(){
  draft={model:$('model').value,prompt:$('prompt').value,account_id:$('account').value,video_id:$('video-id').value};
  try{localStorage.setItem(DRAFT_KEY,JSON.stringify(draft));$('draft-status').textContent='草稿已保存在本机';}
  catch{$('draft-status').textContent='草稿保存失败：本机存储不可用，请复制描述保留。';}
}
function referenceDatabase(){
  if(!draftDB)draftDB=new Promise((resolve,reject)=>{
    const request=indexedDB.open('flow-desktop-drafts',1);
    request.onupgradeneeded=()=>request.result.createObjectStore('references');
    request.onsuccess=()=>resolve(request.result);request.onerror=()=>reject(request.error);
  });
  return draftDB;
}
async function saveReferences(){
  const revision=++referenceRevision,images=[...state.images];
  try{
    const db=await referenceDatabase();
    // A newer change owns the draft if the database was still opening.
    if(revision!==referenceRevision)return;
    await new Promise((resolve,reject)=>{
      const tx=db.transaction('references','readwrite');tx.objectStore('references').put(images,'current');
      tx.oncomplete=resolve;tx.onerror=()=>reject(tx.error);tx.onabort=()=>reject(tx.error);
    });
  }catch{$('draft-status').textContent='参考图未能保存到草稿，重开应用后需重新选择。';}
}
async function restoreDraft(){
  $('prompt').value=typeof draft.prompt==='string'?draft.prompt:'';
  $('video-id').value=typeof draft.video_id==='string'?draft.video_id:'';
  try{
    const db=await referenceDatabase();
    const images=await new Promise((resolve,reject)=>{
      const req=db.transaction('references').objectStore('references').get('current');
      req.onsuccess=()=>resolve(req.result);req.onerror=()=>reject(req.error);
    });
    if(Array.isArray(images))state.images=images.filter(x=>typeof x==='string'&&x.startsWith('data:image/'));
  }catch{$('draft-status').textContent='参考图草稿不可用，请重新选择图片。';}
  renderReferences();
}
for(const id of ['prompt','video-id'])$(id).oninput=()=>{saveDraft();clearComposeError();};
$('account').onchange=()=>{saveDraft();updateReadiness();};
$('prompt').onkeydown=event=>{
  if((event.ctrlKey||event.metaKey)&&event.key==='Enter'&&!event.isComposing){event.preventDefault();if(!$('generate').disabled)$('generate').click();}
};
$('prompt').placeholder+='（⌘ / Ctrl + Enter 提交）';
$('clear-draft').onclick=async()=>{
  if(($('prompt').value||state.images.length||$('video-id').value)&&!confirm('清空当前描述、参考图和源视频？已生成的作品不会删除。'))return;
  $('prompt').value='';$('video-id').value='';state.images=[];$('images').value='';clearComposeError();renderReferences();saveDraft();await saveReferences();
};
function renderReferences(){
  $('references').replaceChildren(...state.images.map((source,index)=>{
    const item=node('div',undefined,'reference-item'),image=node('img');image.src=source;image.alt=`参考图 ${index+1}`;
    const remove=button(`移除第 ${index+1} 张`,async()=>{state.images.splice(index,1);renderReferences();saveDraft();await saveReferences();modelInfo();clearComposeError();});
    item.append(image,remove);return item;
  }));
}
async function fillStudioFromRecord(promptText,modelId,videoId,request=null){
  if(($('prompt').value||state.images.length||$('video-id').value)&&!confirm('用这条记录替换当前草稿？'))return;
  let model=state.models.find(m=>m.id===modelId);
  if(videoId)model=state.models.find(m=>m.capabilities.requires_video_id&&m.capabilities.family===model?.capabilities.family)||state.models.find(m=>m.capabilities.requires_video_id);
  if(model)applyPicker(FlowStudio.selection(model));
  $('prompt').value=promptText||'';$('video-id').value=videoId||'';
  state.images=request?.images?[...request.images]:[];
  $('images').value='';
  $('account').value=request?.account_id&&state.accounts.some(a=>a.id===request.account_id)?request.account_id:'';
  modelInfo();saveDraft();await saveReferences();clearComposeError();page('studio');$('prompt').focus();updateReadiness();
}
async function editTask(task){
  const full=await invoke('task_detail',{id:task.id});
  await fillStudioFromRecord(full.request.prompt,full.request.model,full.request.video_id,full.request);
}
function expandablePrompt(text){
  const details=node('details',undefined,'prompt-details');
  details.append(node('summary',text.length>90?text.slice(0,90)+'…（展开完整描述）':text),node('p',text,'full-prompt'));
  return details;
}

function taskCard(task){
  const card=node('article',undefined,'task-card');
  const header=node('header');
  header.append(
    node('span',labels[task.state]||task.state,'state '+task.state),
    node('small',`${task.request.model} · ${new Date(task.created_at).toLocaleString()}`)
  );
  card.append(header,expandablePrompt(task.request.prompt),node('div',task.stage,'task-stage'));
  if(task.state==='running')card.append(node('small','执行中任务无法撤回；超时或失败后请先核对 Flow 结果，再决定是否重新提交。'));
  if(task.error)card.append(node('p',task.error,'task-error'));
  const actions=node('div',undefined,'task-actions');
  for(const media of task.results||[]){
    actions.append(button(media.kind==='image'?'打开图片 ↗':'打开视频 ↗',()=>invoke('open_media',{filename:media.filename})));
    if(media.kind==='video'&&media.media_id){
      actions.append(button('一键续写 ↗',()=>fillStudioFromRecord(task.request.prompt,task.request.model,media.media_id,{...task.request,images:[]})));
    }
  }
  if(task.request.prompt){
    actions.append(button(['failed','interrupted','cancelled'].includes(task.state)?'再次编辑后重试':'再次编辑',()=>editTask(task)));
  }
  if(task.state==='queued')actions.prepend(button('取消排队',async()=>{await invoke('cancel_task',{id:task.id});await refresh();notice('已取消排队，未提交到 Flow。');}));
  if(actions.children.length)card.append(actions);
  return card;
}

const tasksView=createListView({
  key:'tasks',
  hostId:'task-list',
  searchPlaceholder:'搜索提示词、模型、阶段或错误信息…',
  filterLabel:'任务状态',
  filters:[
    {value:'all',label:'全部状态'},
    {value:'running',label:'生成中'},
    {value:'queued',label:'排队中'},
    {value:'succeeded',label:'已保存'},
    {value:'failed',label:'失败'},
    {value:'interrupted',label:'已中断'},
    {value:'cancelled',label:'已取消'},
    {value:'archived',label:'旧版已完成'},
  ],
  matchFilter:(task,filter)=>task.state===filter,
  searchText:task=>[task.id,task.request?.prompt,task.request?.model,task.stage,task.error,labels[task.state]].filter(Boolean).join(' '),
  sorts:[
    {value:'created_at',label:'创建时间',get:t=>t.created_at||''},
    {value:'updated_at',label:'更新时间',get:t=>t.updated_at||''},
    {value:'state',label:'状态',get:t=>labels[t.state]||t.state||''},
    {value:'model',label:'模型',get:t=>t.request?.model||''},
  ],
  emptyText:'还没有生成任务。从创作工作台开始。',
  noMatchText:'没有匹配当前筛选条件的生成任务。',
  revisionKey:items=>items,
  renderItem:taskCard,
});

function renderTasks(){
  const current=state.tasks.find(t=>t.id===state.activeTaskId)||state.tasks.find(t=>['running','queued'].includes(t.state))||state.tasks[0];
  const key=JSON.stringify([current,state.status?.address]);
  if(key!==state.monitorKey){
    state.monitorKey=key;
    const recent=$('recent-tasks');recent.className='';recent.replaceChildren();
    if(current){
      recent.append(taskCard(current));
      const media=current.results?.[0];
      if(media){recent.prepend(mediaCard(media));if(current.results.length>1)recent.append(button(`查看全部 ${current.results.length} 个作品`,()=>page('library')));}
    }else recent.append(node('div','从左侧描述开始。提交后在这里查看进度与作品。','empty'));
  }
  tasksView.update(state.tasks);
}

async function bindAccountProfile(account,dataDir,profile){
  if(!confirm('请先正常退出使用这份资料的 Chrome。应用将直接使用并更新它，退出应用时关闭它启动的浏览器。新版 Chrome 可能限制日常资料连接；连接失败时可改为独立登录。是否使用？'))return false;
  await invoke('configure_account_browser',{id:account.id,dataDir,profile});
  await refresh();
  notice('已选择已有资料。打开登录窗口后完成登录即可自动同步。');
  return true;
}

async function chooseAccountProfile(account){
  const profiles=await invoke('browser_profiles');
  const dialog=node('dialog',undefined,'profile-dialog');
  dialog.setAttribute('aria-labelledby','profile-dialog-title');
  const heading=node('h2','选择已有的 Chrome 登录资料');
  heading.id='profile-dialog-title';
  dialog.append(heading,node('p','选择你平时使用的资料，无需查找文件夹。资料中的账号名称仅用于辨认，登录状态仍需同步校验。','hint'));
  const list=node('div',undefined,'profile-options');
  const matching=profiles.filter(p=>p.email.toLowerCase()===account.email.toLowerCase());
  const ordered=[...matching,...profiles.filter(p=>!matching.includes(p))];
  for(const profile of ordered){
    const entry=button('',async()=>{if(await bindAccountProfile(account,profile.data_dir,profile.profile))dialog.close();});
    entry.append(node('strong',profile.name+(matching.includes(profile)?' · 与当前邮箱匹配':'')),node('span',profile.email||'Chrome 未记录账号名称'),node('small','Chrome · '+profile.profile));
    list.append(entry);
  }
  if(!profiles.length)list.append(node('p','未找到标准位置的 Chrome 资料。若资料在其他位置，可手动选择；也可以继续使用独立登录。','empty'));
  const controls=node('div',undefined,'actions');
  controls.append(button('取消',()=>dialog.close()),button('其他位置：手动选择…',async()=>{
    const dataDir=await invoke('choose_legacy_directory');if(!dataDir)return;
    const profile=prompt('填写资料子目录名称，例如 Default 或 Profile 1','Default');if(!profile)return;
    if(await bindAccountProfile(account,dataDir,profile))dialog.close();
  }));
  dialog.append(list,controls);
  dialog.addEventListener('close',()=>dialog.remove(),{once:true});
  document.body.append(dialog);
  dialog.showModal();
}

function accountReady(a){
  return a.enabled&&Boolean(a.last_synced)&&(!a.browser_bridge||state.connections.some(c=>c.account_id===a.id&&c.connected&&c.state==='ready'));
}

function accountCard(account){
  const row=node('article',undefined,'account-card');
  const info=node('div');
  const connection=state.connections.find(c=>c.account_id===account.id);
  const online=!account.browser_bridge||Boolean(connection?.connected&&connection.state==='ready');
  info.append(
    node('strong',account.email),
    node('p',`${account.enabled?'已启用':'已停用'} · ${account.last_synced&&online?'同步于 '+new Date(account.last_synced).toLocaleString():'待登录同步'}${account.credits===null?'':' · '+account.credits+' credits'}`)
  );
  const binding=node('div',undefined,'hint');
  binding.textContent=account.browser_bridge?(connection?.connected?connection.message:'默认浏览器连接已断开，请打开 Flow 标签页；若未自动恢复，请重新获取连接码'):account.browser_data_dir?`已有资料：${account.browser_data_dir} · ${account.browser_profile}`:'独立登录：应用自动创建并保留资料';
  info.append(binding);
  const actions=node('div',undefined,'actions');
  actions.append(
    button(account.browser_bridge?'打开 Flow 页面':'打开登录窗口',async()=>{
      if(account.browser_bridge){await invoke('open_flow_page');return;}
      await invoke('open_login',{id:account.id});
      notice('请在专用 Chrome 中登录 '+account.email+'，完成后会自动检测并同步（也可点“同步登录”）。');
    },'primary'),
    button('同步登录',async()=>{
      await invoke('sync_account',{id:account.id});
      await refresh();
      notice('登录账号与余额已同步。');
    }),
    button(account.enabled?'停用':'启用',async()=>{
      await invoke('save_account',{id:account.id,email:account.email,enabled:!account.enabled});
      await refresh();
    }),
    ...(!account.browser_bridge?[button('选择已有登录资料',()=>chooseAccountProfile(account)),
    button('改为独立登录',async()=>{
      await invoke('configure_account_browser',{id:account.id,dataDir:'',profile:''});
      await refresh();
      notice('已切换独立资料，请打开登录窗口完成登录。');
    })]:[]),
    button('删除账号',async()=>{
      if(!confirm('删除该账号的应用登记？浏览器资料会保留在本机。'))return;
      await invoke('delete_account',{id:account.id});
      await refresh();
    },'danger')
  );
  const more=node('details',undefined,'account-more');more.append(node('summary','更多操作'));
  const secondary=node('div');while(actions.children.length>2)secondary.append(actions.children[2]);
  if(secondary.children.length){more.append(secondary);actions.append(more);}
  row.append(info,actions);
  return row;
}

const accountsView=createListView({
  key:'accounts',
  hostId:'account-list',
  searchPlaceholder:'搜索账号邮箱、资料目录或项目 ID…',
  filterLabel:'账号状态',
  filters:[
    {value:'all',label:'全部账号'},
    {value:'ready',label:'已就绪（已同步）'},
    {value:'unsynced',label:'待登录同步'},
    {value:'disabled',label:'已停用'},
  ],
  matchFilter:(a,filter)=>{
    if(filter==='ready')return accountReady(a);
    if(filter==='unsynced')return a.enabled&&!accountReady(a);
    if(filter==='disabled')return !a.enabled;
    return true;
  },
  searchText:a=>[a.email,a.project_id,a.browser_data_dir,a.browser_profile].filter(Boolean).join(' '),
  sorts:[
    {value:'email',label:'邮箱',get:a=>a.email||''},
    {value:'last_synced',label:'同步时间',get:a=>a.last_synced||''},
    {value:'credits',label:'余额',get:a=>a.credits??-1},
  ],
  defaultOrder:'asc',
  emptyText:'打开 Flow 页面，获取连接码并在页面连接当前账号。',
  noMatchText:'没有匹配当前筛选条件的 Google 账号。',
  revisionKey:items=>[items,state.connections],
  renderItem:accountCard,
});

function renderAccounts(){
  const select=$('account');
  const previous=select.value||draft.account_id||'';
  const signature=JSON.stringify(state.accounts.map(a=>[a.id,a.email,accountReady(a)]));
  if(select.dataset.signature!==signature){
    select.dataset.signature=signature;
    const automatic=node('option','自动选择就绪账号');automatic.value='';
    select.replaceChildren(automatic,...state.accounts.map(account=>{const option=node('option',account.email+(accountReady(account)?'':' · 未就绪'));option.value=account.id;return option;}));
    select.value=state.accounts.some(a=>a.id===previous)?previous:'';
  }
  accountsView.update(state.accounts);updateReadiness();
  $('account').closest('label').hidden=state.accounts.length<2;
}

const preferredEmails=['account1@example.com','account2@example.com','account3@example.com'];
for(const email of preferredEmails){
  $('preferred-emails').append(button(email,()=>{
    $('email').value=email;
    $('email').focus();
  }));
}

function mediaCard(media){
  const card=node('article',undefined,'media-card');
  const preview=node(media.kind==='image'?'img':'video');
  if(state.status?.address){
    preview.src=state.status.address+'/media/'+media.filename;
    if(media.kind==='video'){
      preview.controls=true;
      preview.preload='metadata';
    }else{
      preview.alt=media.prompt||'本机生成作品';
      preview.loading='lazy';
    }
  }
  if(!state.status?.address)preview.hidden=true;
  const info=node('div');
  if(!state.status?.address)info.append(node('p','启动本机 API 可预览；仍可打开本机文件。','hint'));
  info.append(node('small',`${media.kind==='image'?'图片':'视频'} · ${(media.size/1024/1024).toFixed(1)} MB · ${new Date(media.created_at).toLocaleString()}`));
  if(media.prompt)info.append(expandablePrompt(media.prompt));
  const actions=node('div',undefined,'actions');
  actions.append(
    button('打开文件 ↗',()=>invoke('open_media',{filename:media.filename})),
    button('复制链接',async()=>{
      if(!state.status?.address)throw Error('请先启动 API');
      await copy(state.status.address+'/media/'+media.filename);
    })
  );
  if(media.prompt||media.model){
    actions.append(button('填入工作台',()=>fillStudioFromRecord(media.prompt,media.model,null)));
  }
  if(media.kind==='image')actions.append(button('用作参考图',async()=>{
    const source=await invoke('media_reference',{filename:media.filename});
    if(!selectedModel()?.capabilities.supports_images){
      const model=state.models.find(m=>m.id==='gemini-3.1-flash-lite-image-square');if(model)applyPicker(FlowStudio.selection(model));
    }
    const max=selectedModel()?.capabilities.max_images||0;
    if(state.images.length>=max)throw Error(`当前模型最多支持 ${max} 张参考图，请先移除不需要的图片。`);
    state.images.push(source);renderReferences();saveDraft();await saveReferences();page('studio');notice('已加入参考图，填写修改要求后即可生成。');
  }));
  if(media.media_id){
    actions.append(button('复制 media ID',()=>copy(media.media_id)));
    if(media.kind==='video'){
      actions.append(button('一键续写 ↗',()=>fillStudioFromRecord(media.prompt,media.model,media.media_id,{account_id:state.tasks.find(t=>t.id===media.task_id)?.request.account_id,images:[]}),'primary'));
    }
  }
  info.append(actions);
  card.append(preview,info);
  return card;
}

const mediaView=createListView({
  key:'library',
  hostId:'media-grid',
  bodyClass:'media-grid-items',
  searchPlaceholder:'搜索作品提示词、模型、文件名或 media ID…',
  filterLabel:'作品类型',
  filters:[
    {value:'all',label:'全部类型'},
    {value:'image',label:'图片'},
    {value:'video',label:'视频'},
  ],
  matchFilter:(m,filter)=>m.kind===filter,
  searchText:m=>[m.prompt,m.model,m.filename,m.media_id,m.kind].filter(Boolean).join(' '),
  sorts:[
    {value:'created_at',label:'创建时间',get:m=>m.created_at||''},
    {value:'size',label:'文件大小',get:m=>Number(m.size||0)},
    {value:'kind',label:'类型',get:m=>m.kind||''},
  ],
  emptyText:'作品生成成功后会自动出现在这里。',
  noMatchText:'没有匹配当前筛选条件的作品。',
  revisionKey:items=>[state.status?.address,items],
  renderItem:mediaCard,
});

function renderMedia(){
  $('success-count').textContent=state.media.length;
  mediaView.update(state.media);
}

async function reloadSettings(){
  state.settings=await invoke('settings');
  for(const [key,value]of Object.entries(state.settings)){
    const input=$('settings-form').elements.namedItem(key);
    if(input){if(input.type==='checkbox')input.checked=Boolean(value);else input.value=value;}
  }
  $('base-url').textContent=`http://127.0.0.1:${state.settings.port}/v1`;
  await loadMagpie();
}

async function loadMagpie(){
  try{
    const info=await invoke('magpie_status');
    $('magpie-status').textContent=info.configured
      ?`已接入 Magpie · ${info.models} 个图片规格 · ${info.base_url}`
      :'尚未接入当前设置，或端口 / Key 已变更。点击一键接入即可更新。';
  }catch(error){$('magpie-status').textContent=String(error);}
}

async function refresh(){
  const [accounts,tasks,media,status,connections]=await Promise.all([
    invoke('accounts'),
    invoke('tasks'),
    invoke('library'),
    invoke('status'),
    invoke('browser_connections'),
  ]);
  state.connections=connections;
  state.accounts=accounts;
  state.tasks=tasks;
  state.media=media;
  updateStatus(status);
  renderAccounts();
  renderTasks();
  renderMedia();
}

$('toggle-service').onclick=()=>action($('toggle-service'),async()=>{
  await invoke(state.status?.service==='running'?'stop_service':'start_service');
  await refresh();
});
$('choose-browser').onclick=()=>action($('choose-browser'),async()=>{
  const path=await invoke('choose_browser');
  if(path)$('settings-form').elements.namedItem('browser_executable').value=path;
  notice('已填入浏览器路径，点击“保存并生效”自动应用。');
});
$('quick-login').onclick=()=>action($('quick-login'),async()=>{
  await invoke('quick_login');
  notice('已打开 Chrome，请登录 Google 账号。登录完成后应用会自动识别邮箱并同步，无需其他操作。');
});
$('open-flow').onclick=()=>action($('open-flow'),()=>invoke('open_flow_page'));
$('pair-browser').onclick=()=>action($('pair-browser'),async()=>{
  const code=await invoke('browser_pairing_code');
  $('pairing-code').textContent=`连接码：${code} · 5 分钟有效，复制到 Flow 页面右下角。`;
  await copy(code);
});
$('open-userscript').onclick=()=>action($('open-userscript'),async()=>{
  await invoke('open_userscript_url');
  notice('已在浏览器中打开油猴脚本安装页面。若已安装 Tampermonkey 扩展，将直接弹出安装提示。');
});
$('download-chrome').onclick=()=>action($('download-chrome'),()=>invoke('open_chrome_download'));

$('add-account').onsubmit=event=>{
  event.preventDefault();
  const b=event.submitter||$('add-account').querySelector('button[type=submit]');
  const openNow=b?.dataset?.openLogin!=='false';
  action(b,async()=>{
    const email=$('email').value.trim();
    const existing=state.accounts.find(a=>a.email.toLowerCase()===email.toLowerCase());
    const account=existing||await invoke('save_account',{id:null,email,enabled:true});
    $('email').value='';
    await refresh();
    if(openNow){
      await invoke('open_login',{id:account.id});
      notice(`已添加并打开 ${account.email} 的 Chrome 登录窗口，登录完成后将自动检测同步。`);
    }else{
      notice('账号已添加。打开登录窗口后，在 Chrome 登录即可自动同步。');
    }
  });
};

$('images').onchange=async()=>{
  const files=[...$('images').files];
  if(!files.length)return;
  state.loadingImages=true;updateReadiness();
  try{
    const max=selectedModel()?.capabilities.max_images||0;
    if(state.images.length+files.length>max)throw Error(`当前模型最多支持 ${max} 张参考图。请先移除不需要的图片。`);
    if(files.some(file=>!['image/png','image/jpeg','image/webp'].includes(file.type)))throw Error('请选择 PNG、JPEG 或 WebP 图片。');
    if(files.some(file=>file.size>20*1024*1024))throw Error('单张参考图不能超过 20 MB');
    const images=await Promise.all(files.map(file=>new Promise((resolve,reject)=>{
      const reader=new FileReader();reader.onload=()=>resolve(reader.result);reader.onerror=()=>reject(Error('无法读取图片，请重新选择。'));reader.readAsDataURL(file);
    })));
    state.images.push(...images);renderReferences();clearComposeError();saveDraft();await saveReferences();
  }catch(error){notice(error,true);}
  finally{$('images').value='';state.loadingImages=false;modelInfo();updateReadiness();}
};

$('generate').onclick=async()=>{
  if(state.submitting||$('generate').disabled)return;
  clearComposeError();
  const request=studioRequest(),error=FlowStudio.validate(request,selectedModel());
  if(error){showComposeError(error);return;}
  state.submitting=true;updateReadiness();
  try{
    const task=await invoke('submit',{request});state.activeTaskId=task.id;
    saveDraft();await refresh();notice('任务已提交，进度和作品会显示在右侧。');
  }catch(error){notice(error,true);}
  finally{state.submitting=false;updateReadiness();}
};

for(const id of ['open-data','settings-open-data'])$(id).onclick=()=>action($(id),()=>invoke('open_data'));
$('quit').onclick=()=>action($('quit'),()=>invoke('quit'));
document.querySelectorAll('.refresh').forEach(b=>b.onclick=()=>action(b,refresh));

async function copy(text){
  await navigator.clipboard.writeText(text);
  notice('已复制');
}
$('copy-base').onclick=()=>action($('copy-base'),()=>copy($('base-url').textContent));
$('copy-key').onclick=()=>action($('copy-key'),()=>copy(state.settings.api_key));
$('connect-magpie').onclick=()=>action($('connect-magpie'),async()=>{
  try{
    const info=await invoke('connect_magpie');
    await refresh();
    $('magpie-status').textContent=`已接入 Magpie · ${info.models} 个图片规格 · ${info.base_url}。`
      +(info.backup_path?`原配置备份：${info.backup_path}。`:'配置已是最新。')
      +(state.status?.ready_accounts?'可以通过 hzr-flow2api-image 生成图片。':'请在 Google 账号页恢复浏览器连接后生图。');
    notice('Magpie 接入配置已保存。');
  }catch(error){$('magpie-status').textContent=String(error);throw error;}
});

$('settings-form').onsubmit=event=>{
  event.preventDefault();
  action(event.submitter,async()=>{
    const form=new FormData(event.currentTarget);
    const settings=Object.fromEntries(form);
    settings.auto_update=form.has('auto_update');
    for(const name of ['port','max_concurrent','image_timeout_seconds','video_timeout_seconds'])settings[name]=Number(settings[name]);
    await invoke('save_settings',{settings});
    state.settings=settings;
    $('base-url').textContent=`http://127.0.0.1:${settings.port}/v1`;
    await loadMagpie();
    await refresh();
    notice(state.status?.service==='running'?'设置已保存并自动重启 API 生效。':'设置已保存。');
  });
};

function showWebdav(config){
  $('webdav-url').value=config.url;
  $('webdav-user').value=config.username;
  $('webdav-secret').value='';
  $('webdav-secret').placeholder=config.has_password?'已保存密码，留空则保留':'坚果云第三方应用密码';
  $('webdav-status').textContent=config.last_result||'尚未同步。应用密码只保存在系统凭据库。';
}

async function loadDevice(){
  try{$('autostart').checked=await invoke('autostart_enabled');}catch(error){$('autostart').checked=false;notice(error,true);}
  showWebdav(await invoke('webdav_config'));
}

$('autostart').onchange=async()=>{
  const want=$('autostart').checked;
  $('autostart').disabled=true;
  try{
    const actual=await invoke('set_autostart',{enabled:want});
    $('autostart').checked=actual;
    notice(actual?'已打开登录后自动启动':'已关闭登录后自动启动');
  }catch(error){
    try{$('autostart').checked=await invoke('autostart_enabled');}catch{$('autostart').checked=false;}
    notice(error,true);
  }finally{
    $('autostart').disabled=false;
  }
};

$('webdav-save').onclick=()=>action($('webdav-save'),async()=>{
  showWebdav(await invoke('save_webdav',{url:$('webdav-url').value.trim(),username:$('webdav-user').value.trim(),secret:$('webdav-secret').value}));
  notice('WebDAV 连接已保存。');
});
$('webdav-import').onclick=()=>action($('webdav-import'),async()=>{
  showWebdav(await invoke('import_verge_webdav'));
  notice('已从本机 Clash Verge 导入连接。密码没有显示。');
});
for(const [id,name] of [['webdav-test','test'],['webdav-push','push'],['webdav-pull','pull']]){
  $(id).onclick=()=>action($(id),async()=>{
    const message=await invoke('webdav_action',{action:name});
    if(name==='pull'){
      await reloadSettings();
      await refresh();
    }
    showWebdav(await invoke('webdav_config'));
    notice(message);
  });
}

function renderUpdateStatus(status){
  if(!status?.message)return;
  const checked=status.checked_at?` 上次检查：${new Date(status.checked_at*1000).toLocaleString()}。`:'';
  $('update-status').textContent=status.message+checked;
  const dialogStatus=document.getElementById('dialog-update-status');
  if(dialogStatus)dialogStatus.textContent=status.message+checked;
  if($('update-badge'))$('update-badge').textContent=({error:'更新失败',checking:'检查中',current:'已是最新',available:'有新版本',waiting:'等待空闲',downloading:'下载中',installing:'安装中'})[status.phase]||'检查更新';
}

let pendingUpdate=null;

function showUpdateDialog(autoCheck=false){
  let dialog=document.getElementById('update-dialog');
  if(!dialog){
    dialog=node('dialog',undefined,'profile-dialog update-dialog');
    dialog.id='update-dialog';
    dialog.setAttribute('aria-labelledby','update-dialog-title');
    const header=node('div',undefined,'section-heading');
    const headingWrap=node('div');
    const title=node('h2','软件版本与更新');
    title.id='update-dialog-title';
    headingWrap.append(title,node('p',`当前运行版本：v${APP_VERSION}`,'hint'));
    const closeBtn=button('关闭',()=>dialog.close(),'quiet');
    closeBtn.style.fontSize='16px';
    header.append(headingWrap,closeBtn);

    const body=node('div');
    body.style.margin='20px 0';
    const statusText=node('p','点击“立即检查”将通过本机已登录的 GitHub CLI 检查私有仓库最新版本。','hint');
    statusText.id='dialog-update-status';
    const releaseNotes=node('pre',undefined,'update-notes');
    releaseNotes.id='dialog-update-notes';
    releaseNotes.style.display='none';
    body.append(statusText,releaseNotes);

    const actions=node('div',undefined,'actions');
    const checkBtn=button('立即检查 ↗',()=>runUpdateCheck(checkBtn),'secondary');
    checkBtn.id='dialog-check-btn';
    const installBtn=button('立即安装并重启',()=>runInstallUpdate(installBtn),'primary');
    installBtn.id='dialog-install-btn';
    installBtn.style.display='none';
    const cancelBtn=button('关闭',()=>dialog.close(),'secondary');
    actions.append(checkBtn,installBtn,cancelBtn);

    dialog.append(header,body,actions);
    dialog.addEventListener('close',()=>dialog.remove(),{once:true});
    document.body.append(dialog);
  }
  dialog.showModal();
  if(autoCheck)runUpdateCheck(document.getElementById('dialog-check-btn'));
}

async function runUpdateCheck(triggerBtn){
  const btns=[triggerBtn,$('check-update'),$('header-check-update'),$('sidebar-check-update'),document.getElementById('dialog-check-btn')].filter(Boolean);
  btns.forEach(b=>b.disabled=true);
  const setStatus=(msg,notes=null)=>{
    $('update-status').textContent=msg;
    const dialogStatus=document.getElementById('dialog-update-status');
    const dialogNotes=document.getElementById('dialog-update-notes');
    if(dialogStatus)dialogStatus.textContent=msg;
    if(dialogNotes){
      if(notes){
        dialogNotes.textContent=notes;
        dialogNotes.style.display='block';
      }else{
        dialogNotes.style.display='none';
      }
    }
  };
  setStatus('正在连接 GitHub 检查最新版本……');
  try{
    const found=await invoke('check_private_update');
    pendingUpdate=found;
    if(!found){
      $('install-update').hidden=true;
      const dialogInstall=document.getElementById('dialog-install-btn');
      if(dialogInstall)dialogInstall.style.display='none';
      const msg=`当前已是最新版本（v${APP_VERSION}）。`;
      setStatus(msg);
      if($('update-badge'))$('update-badge').textContent='已是最新';
      notice(msg);
      return;
    }
    const msg=`发现新版本 v${found.version}（当前 v${found.current_version||APP_VERSION}）。`;
    setStatus(msg,found.body||'');
    $('install-update').hidden=false;
    const dialogInstall=document.getElementById('dialog-install-btn');
    if(dialogInstall)dialogInstall.style.display='inline-block';
    if($('update-badge'))$('update-badge').textContent=`可升级至 v${found.version}`;
    notice(`发现新版本 v${found.version}，可立即更新。`);
  }catch(err){
    const msg='检查更新失败：'+(err?.message||err);
    setStatus(msg);
    notice(msg,true);
  }finally{
    btns.forEach(b=>b.disabled=false);
  }
}

async function runInstallUpdate(triggerBtn){
  if(!pendingUpdate)throw Error('请先检查新版本');
  action(triggerBtn||$('install-update'),async()=>{
    await invoke('install_private_update');
    notice('更新包下载校验完成，正在重启应用……');
  });
}

$('check-update').onclick=()=>runUpdateCheck($('check-update'));
$('install-update').onclick=()=>runInstallUpdate($('install-update'));
if($('header-check-update'))$('header-check-update').onclick=()=>showUpdateDialog(true);
if($('sidebar-check-update'))$('sidebar-check-update').onclick=()=>showUpdateDialog(true);

(async()=>{
  try{
    if(!window.__TAURI__)throw Error('请通过 Flow2API 桌面应用打开此界面');
    await window.__TAURI__.event.listen('status',event=>updateStatus(event.payload));
    await window.__TAURI__.event.listen('notice',event=>notice(event.payload,String(event.payload).startsWith('无法')));
    await window.__TAURI__.event.listen('account-synced',()=>{refresh().catch(()=>{});});
    await window.__TAURI__.event.listen('check_update',()=>{showUpdateDialog(true);});
    await window.__TAURI__.event.listen('private-update-progress',event=>{
      const received=event.payload.received,total=event.payload.total;
      const msg=total?`已下载 ${(received/1048576).toFixed(1)} / ${(total/1048576).toFixed(1)} MB (${Math.round(received/total*100)}%)`:`已下载 ${(received/1048576).toFixed(1)} MB`;
      $('update-status').textContent=msg;
      const dialogStatus=document.getElementById('dialog-update-status');
      if(dialogStatus)dialogStatus.textContent=msg;
    });
    await window.__TAURI__.event.listen('private-update-installing',()=>{
      const msg='正在校验签名并安装，当前版本会保留到安装成功。';
      $('update-status').textContent=msg;
      const dialogStatus=document.getElementById('dialog-update-status');
      if(dialogStatus)dialogStatus.textContent=msg;
    });
    await window.__TAURI__.event.listen('private-update-status',event=>renderUpdateStatus(event.payload));
    renderUpdateStatus(await invoke('private_update_status'));
    await reloadSettings();
    await restoreDraft();
    state.models=(await invoke('models')).data;
    renderModels();
    $('platform').textContent=navigator.userAgent.includes('Mac')?'macOS · 菜单栏运行':navigator.userAgent.includes('Windows')?'Windows · 托盘运行':'Linux · 托盘运行';
    await refresh();
    loadDevice().catch(error=>notice('设备设置读取失败：'+error,true));
    let polling=false;
    setInterval(async()=>{
      if(polling)return;
      polling=true;
      try{await refresh()}catch(error){notice(error,true)}finally{polling=false}
    },4000);
  }catch(error){
    notice(error,true);
  }
})();
