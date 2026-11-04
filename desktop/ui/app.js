/* All privileged operations use local Tauri commands; no credentials go to remote pages. */
'use strict';
const $ = id => document.getElementById(id);
const invoke = (name, args) => window.__TAURI__.core.invoke(name, args);
const state = {status:null,models:[],accounts:[],tasks:[],media:[],legacyRecords:[],settings:null,images:[],page:'studio'};
const labels = {queued:'排队中',running:'生成中',succeeded:'已保存',failed:'失败',interrupted:'已中断',archived:'旧版已完成'};
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
  $('generate').disabled=!running;
  if(status.last_error&&status.service==='failed')notice(status.last_error,true);
}

function selectedModel(){return state.models.find(model=>model.id===$('model').value)}
function modelInfo(){
  const model=selectedModel();
  if(!model)return;
  const c=model.capabilities;
  $('model-info').textContent=`${c.family} · ${c.aspect_ratio}${c.duration_seconds?' · '+c.duration_seconds+' 秒':''} · ${c.summary}`;
  $('video-field').hidden=!c.requires_video_id;
  const price=c.pricing;
  $('model-price').textContent=price.credits===null?'此规格费用未核价，以账号实际扣费为准':`${price.verified_at} 参考：${price.credits} credits / ${price.unit}，以实际扣费为准`;
  $('images').disabled=!c.supports_images;
}
function renderModels(){
  const select=$('model');
  const previous=select.value;
  select.replaceChildren();
  for(const model of state.models){
    const c=model.capabilities;
    const option=node('option',`${c.family} · ${c.aspect_ratio} · ${c.type==='image'?c.resolution:c.duration_seconds+' 秒'}${c.kind==='extend'?' · 续写':''}${c.kind==='i2v'?' · 首尾帧':''}${c.kind==='r2v'?' · 参考图':''}`);
    option.value=model.id;
    select.append(option);
  }
  if(state.models.some(m=>m.id===previous))select.value=previous;
  else select.value='gemini-3.1-flash-lite-image-square';
  modelInfo();
}
$('model').onchange=modelInfo;

function fillStudioFromRecord(promptText,modelId,videoId){
  if(modelId&&state.models.some(m=>m.id===modelId)){
    $('model').value=modelId;
    modelInfo();
  }
  if(promptText){
    $('prompt').value=promptText;
  }
  if(videoId){
    const current=selectedModel();
    if(!current?.capabilities?.requires_video_id){
      const extendModel=state.models.find(m=>m.capabilities?.requires_video_id);
      if(extendModel){
        $('model').value=extendModel.id;
        modelInfo();
      }
    }
    $('video-id').value=videoId;
  }
  page('studio');
  $('prompt').focus();
}

function taskCard(task){
  const card=node('article',undefined,'task-card');
  const header=node('header');
  header.append(
    node('span',labels[task.state]||task.state,'state '+task.state),
    node('small',`${task.request.model} · ${new Date(task.created_at).toLocaleString()}`)
  );
  card.append(header,node('div',task.request.prompt,'prompt'),node('div',task.stage,'task-stage'));
  if(task.error)card.append(node('p',task.error,'task-error'));
  const actions=node('div',undefined,'task-actions');
  for(const media of task.results||[]){
    actions.append(button(media.kind==='image'?'打开图片 ↗':'打开视频 ↗',()=>invoke('open_media',{filename:media.filename})));
    if(media.kind==='video'&&media.media_id){
      actions.append(button('一键续写 ↗',()=>fillStudioFromRecord(task.request.prompt,null,media.media_id)));
    }
  }
  if(task.request.prompt){
    actions.append(button('填入工作台',()=>fillStudioFromRecord(task.request.prompt,task.request.model,task.request.video_id)));
  }
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
  const recent=$('recent-tasks');
  recent.className='';
  recent.replaceChildren(...state.tasks.slice(0,3).map(taskCard));
  if(!state.tasks.length)recent.append(node('div','提交后在这里查看生成进度。','empty'));
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

function accountCard(account){
  const row=node('article',undefined,'account-card');
  const info=node('div');
  info.append(
    node('strong',account.email),
    node('p',`${account.enabled?'已启用':'已停用'} · ${account.last_synced?'同步于 '+new Date(account.last_synced).toLocaleString():'待登录同步'}${account.credits===null?'':' · '+account.credits+' credits'}`)
  );
  const binding=node('div',undefined,'hint');
  binding.textContent=account.browser_data_dir?`已有资料：${account.browser_data_dir} · ${account.browser_profile}`:'独立登录：应用自动创建并保留资料';
  info.append(binding);
  const actions=node('div',undefined,'actions');
  actions.append(
    button('打开登录窗口',async()=>{
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
    button('选择已有登录资料',()=>chooseAccountProfile(account)),
    button('改为独立登录',async()=>{
      await invoke('configure_account_browser',{id:account.id,dataDir:'',profile:''});
      await refresh();
      notice('已切换独立资料，请打开登录窗口完成登录。');
    }),
    button('删除',async()=>{
      if(!confirm('删除该账号的应用登记？浏览器资料会保留在本机。'))return;
      await invoke('delete_account',{id:account.id});
      await refresh();
    })
  );
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
    if(filter==='ready')return a.enabled&&Boolean(a.last_synced);
    if(filter==='unsynced')return a.enabled&&!a.last_synced;
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
  emptyText:'先添加账号，再打开 Chrome 登录并同步。',
  noMatchText:'没有匹配当前筛选条件的 Google 账号。',
  revisionKey:items=>items,
  renderItem:accountCard,
});

function renderAccounts(){
  const select=$('account');
  const previous=select.value;
  select.replaceChildren(node('option','自动分配已登录账号'));
  select.firstChild.value='';
  for(const account of state.accounts.filter(a=>a.enabled&&a.last_synced)){
    const option=node('option',account.email);
    option.value=account.id;
    select.append(option);
  }
  select.value=previous;
  accountsView.update(state.accounts);
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
  const info=node('div');
  info.append(node('small',`${media.kind==='image'?'图片':'视频'} · ${(media.size/1024/1024).toFixed(1)} MB · ${new Date(media.created_at).toLocaleString()}`));
  if(media.prompt)info.append(node('p',media.prompt,'prompt'));
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
  if(media.media_id){
    actions.append(button('复制 media ID',()=>copy(media.media_id)));
    if(media.kind==='video'){
      actions.append(button('一键续写 ↗',()=>fillStudioFromRecord(media.prompt,null,media.media_id),'primary'));
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
    if(input)input.value=value;
  }
  $('base-url').textContent=`http://127.0.0.1:${state.settings.port}/v1`;
}

async function refresh(){
  const [accounts,tasks,media,status]=await Promise.all([
    invoke('accounts'),
    invoke('tasks'),
    invoke('library'),
    invoke('status'),
  ]);
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
$('install-extension').onclick=()=>action($('install-extension'),async()=>{
  const path=await invoke('install_chrome_extension');
  notice(`已为你启动 Chrome 并自动挂载扩展（目录：${path}）。\n若需永久安装，可开启 chrome://extensions 开发者模式后将该目录拖入。`);
});
$('open-userscript').onclick=()=>action($('open-userscript'),async()=>{
  await invoke('open_userscript_url');
  notice('已在浏览器中打开油猴脚本安装页面。若已安装 Tampermonkey 扩展，将直接弹出安装提示。');
});
$('open-extension-dir').onclick=()=>action($('open-extension-dir'),async()=>{
  const path=await invoke('open_extension_folder');
  notice(`已打开扩展目录：${path}。\n在 Chrome 打开 chrome://extensions 并开启「开发者模式」，拖入此目录即可。`);
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
  try{
    const files=[...$('images').files];
    const max=selectedModel()?.capabilities.max_images||0;
    if(files.length>max)throw Error(`当前模型最多支持 ${max} 张参考图`);
    if(files.some(file=>file.size>20*1024*1024))throw Error('单张参考图不能超过 20 MB');
    state.images=await Promise.all(files.map(file=>new Promise((resolve,reject)=>{
      const reader=new FileReader();
      reader.onload=()=>resolve(reader.result);
      reader.onerror=reject;
      reader.readAsDataURL(file);
    })));
    $('references').replaceChildren(...state.images.map(source=>{
      const image=node('img');
      image.src=source;
      image.alt='参考图';
      return image;
    }));
  }catch(error){
    state.images=[];
    $('images').value='';
    $('references').replaceChildren();
    notice(error,true);
  }
};

$('generate').onclick=()=>action($('generate'),async()=>{
  const model=selectedModel();
  if(!model)throw Error('请选择模型');
  if(!state.accounts.some(a=>a.enabled&&a.last_synced)){
    page('accounts');
    throw Error('请先添加账号并完成登录同步');
  }
  await invoke('submit',{request:{model:model.id,prompt:$('prompt').value,images:state.images,account_id:$('account').value||null,video_id:$('video-id').value||null}});
  await refresh();
  notice('任务已提交，可以关闭窗口在后台继续执行。');
});

for(const id of ['open-data','settings-open-data'])$(id).onclick=()=>action($(id),()=>invoke('open_data'));
$('quit').onclick=()=>action($('quit'),()=>invoke('quit'));
document.querySelectorAll('.refresh').forEach(b=>b.onclick=()=>action(b,refresh));

async function copy(text){
  await navigator.clipboard.writeText(text);
  notice('已复制');
}
$('copy-base').onclick=()=>action($('copy-base'),()=>copy($('base-url').textContent));
$('copy-key').onclick=()=>action($('copy-key'),()=>copy(state.settings.api_key));

$('settings-form').onsubmit=event=>{
  event.preventDefault();
  action(event.submitter,async()=>{
    const form=new FormData(event.currentTarget);
    const settings=Object.fromEntries(form);
    for(const name of ['port','max_concurrent','image_timeout_seconds','video_timeout_seconds'])settings[name]=Number(settings[name]);
    await invoke('save_settings',{settings});
    state.settings=settings;
    $('base-url').textContent=`http://127.0.0.1:${settings.port}/v1`;
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

let pendingUpdate=null;
$('check-update').onclick=()=>action($('check-update'),async()=>{
  const found=await invoke('check_private_update');
  pendingUpdate=found;
  if(!found){
    $('install-update').hidden=true;
    $('update-status').textContent='已是当前版本。';
    notice('已是当前版本');
    return;
  }
  $('update-status').textContent=`可以安装 ${found.version}（当前 ${found.current_version}）。${found.body||''}`;
  $('install-update').hidden=false;
});
$('install-update').onclick=()=>action($('install-update'),async()=>{
  if(!pendingUpdate)throw Error('请先检查新版本');
  await invoke('install_private_update');
  notice('更新已安装。');
});

(async()=>{
  try{
    if(!window.__TAURI__)throw Error('请通过 Flow2API 桌面应用打开此界面');
    await window.__TAURI__.event.listen('status',event=>updateStatus(event.payload));
    await window.__TAURI__.event.listen('notice',event=>notice(event.payload,String(event.payload).startsWith('无法')));
    await window.__TAURI__.event.listen('account-synced',()=>{refresh().catch(()=>{});});
    await window.__TAURI__.event.listen('private-update-progress',event=>{
      const received=event.payload.received,total=event.payload.total;
      $('update-status').textContent=total?`已下载 ${received} / ${total} 字节`:`已下载 ${received} 字节`;
    });
    await window.__TAURI__.event.listen('private-update-installing',()=>{
      $('update-status').textContent='正在校验并安装，当前版本会保留到安装成功。';
    });
    await reloadSettings();
    state.models=(await invoke('models')).data;
    renderModels();
    $('platform').textContent=navigator.userAgent.includes('Mac')?'macOS · 菜单栏运行':'Windows · 托盘运行';
    await loadDevice();
    await refresh();
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
