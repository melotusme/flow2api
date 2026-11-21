/* All privileged operations use local Tauri commands; no credentials go to remote pages. */
'use strict';
let APP_VERSION='0.4.0';
const $ = id => document.getElementById(id);
const invoke = (name, args) => window.__TAURI__.core.invoke(name, args);
const state = {status:null,models:[],accounts:[],tasks:[],media:[],legacyRecords:[],settings:null,images:[],page:'studio',connections:[],musicConnections:[],submitting:false,loadingImages:false,activeTaskId:null,monitorKey:''};
const labels = {queued:'排队中',running:'生成中',succeeded:'已保存',failed:'失败',interrupted:'已中断',cancelled:'已取消',archived:'旧版已完成'};
const titles = {studio:'创作工作台',tasks:'任务与进度',library:'本机作品',accounts:'Google 账号',settings:'设置与接入'};
let noticeTimer=null,noticeSticky=false,noticePaused=false;

function announce(message,urgent=false){
  const target=$(urgent?'sr-alert':'sr-status');
  if(!target)return;
  target.textContent='';
  requestAnimationFrame(()=>{target.textContent=String(message);});
}
function scheduleNoticeDismiss(){
  clearTimeout(noticeTimer);
  if(noticeSticky||noticePaused)return;
  noticeTimer=setTimeout(()=>{$('notice').hidden=true;},6500);
}
const noticeBox=$('notice');
noticeBox.addEventListener('mouseenter',()=>{noticePaused=true;clearTimeout(noticeTimer);});
noticeBox.addEventListener('mouseleave',()=>{noticePaused=false;scheduleNoticeDismiss();});
noticeBox.addEventListener('focusin',()=>{noticePaused=true;clearTimeout(noticeTimer);});
noticeBox.addEventListener('focusout',()=>{noticePaused=false;scheduleNoticeDismiss();});

function notice(message,error=false){
  const text=String(message);
  noticeSticky=Boolean(error);
  noticePaused=false;
  noticeBox.replaceChildren(
    node('span',text,'notice-text'),
    button('关闭',()=>{noticeBox.hidden=true;clearTimeout(noticeTimer);},'text-button notice-close')
  );
  noticeBox.className=error?'error':'';
  noticeBox.hidden=false;
  announce(text,Boolean(error));
  scheduleNoticeDismiss();
}
function node(tag,text,className){
  const element=document.createElement(tag);
  if(text!==undefined)element.textContent=text;
  if(className)element.className=className;
  return element;
}
function setAriaDisabled(el,disabled){
  if(!el)return;
  if(disabled)el.setAttribute('aria-disabled','true');
  else el.removeAttribute('aria-disabled');
}
async function action(control,callback){
  if(!control)return callback();
  if(control.dataset.busy==='1'||control.getAttribute('aria-disabled')==='true')return;
  control.dataset.busy='1';
  setAriaDisabled(control,true);
  try{return await callback()}catch(error){notice(error,true)}finally{
    delete control.dataset.busy;
    setAriaDisabled(control,false);
  }
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
  search.id=`list-search-${config.key}`;
  search.name=`list_search_${config.key}`;
  search.placeholder=config.searchPlaceholder||'关键字筛选…';
  search.setAttribute('aria-label',config.searchPlaceholder||'关键字筛选');
  search.oninput=()=>{view.query=search.value.trim().toLowerCase();view.page=1;render(true,true);};
  toolbar.append(search);

  let filterSelect=null;
  if(config.filters&&config.filters.length){
    filterSelect=node('select');
    filterSelect.id=`list-filter-${config.key}`;
    filterSelect.name=`list_filter_${config.key}`;
    filterSelect.setAttribute('aria-label',config.filterLabel||'维度筛选');
    for(const item of config.filters){
      const opt=node('option',item.label);
      opt.value=item.value;
      filterSelect.append(opt);
    }
    filterSelect.onchange=()=>{view.filter=filterSelect.value;view.page=1;render(true,true);};
    toolbar.append(filterSelect);
  }

  const sortSelect=node('select');
  sortSelect.id=`list-sort-${config.key}`;
  sortSelect.name=`list_sort_${config.key}`;
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
    sizes.id=`list-size-${config.key}`;
    sizes.name=`list_size_${config.key}`;
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

  function render(force=false,announceResult=false){
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
    const hideToolbar=Boolean(config.autoHideToolbar&&raw.length<config.autoHideToolbar&&!view.query&&view.filter==='all');
    toolbar.hidden=hideToolbar;
    const hidePager=Boolean(config.autoHidePager&&pages<=1&&total<=view.pageSize);
    pager.hidden=hidePager;
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
    if(!hidePager)renderPager(total,pages);
    if(announceResult){
      const summaryText=!raw.length?(config.emptyText||'暂无记录。'):!total?(config.noMatchText||'没有匹配筛选条件的记录。'):`已筛选出 ${total} 条记录，当前第 ${view.page} / ${pages} 页。`;
      announce(summaryText,false);
    }
  }

  return {
    update(items,force=false){
      view.items=Array.isArray(items)?items:[];
      render(force);
    },
    refresh(){render(true);},
  };
}

const settingsNav=document.querySelector('.settings-nav');
const settingsLinks=[...settingsNav.querySelectorAll('a')];
const settingsSections=settingsLinks.map(link=>document.getElementById(link.hash.slice(1)));
let settingsProgrammaticId=null,settingsProgrammaticTimer=0;
function highlightSettingsSection(id){
  settingsLinks.forEach(link=>{
    if(link.hash===`#${id}`)link.setAttribute('aria-current','location');
    else link.removeAttribute('aria-current');
  });
}

function page(name,{focus=true}={}){
  const changed=state.page!==name;
  state.page=name;
  document.documentElement.dataset.page=name;
  document.querySelectorAll('.page').forEach(element=>element.hidden=element.id!==name);
  if($('summary'))$('summary').hidden=!['studio','tasks'].includes(name);
  document.querySelectorAll('[data-page]').forEach(element=>{
    const active=element.dataset.page===name;
    element.classList.toggle('active',active);
    if(active)element.setAttribute('aria-current','page');
    else element.removeAttribute('aria-current');
  });
  $('page-title').textContent=titles[name];
  document.title=titles[name]+' · Flow2API';
  if(changed){
    settingsProgrammaticId=null;
    clearTimeout(settingsProgrammaticTimer);
    window.scrollTo({top:0,behavior:'instant'});
    if(name==='settings')highlightSettingsSection(settingsSections[0]?.id||'settings-runtime');
  }
  if(focus)$('page-title').focus({preventScroll:true});
}
page(state.page,{focus:false});
document.querySelectorAll('[data-page]').forEach(element=>element.onclick=event=>page(element.dataset.page,{focus:event.detail===0}));
document.querySelectorAll('[data-go]').forEach(element=>element.onclick=event=>page(element.dataset.go,{focus:event.detail===0}));

let settingsScrollFrame=0;
function syncSettingsNavigation(){
  settingsScrollFrame=0;
  if(state.page!=='settings'||settingsProgrammaticId)return;
  const edge=Math.max(96,settingsNav.getBoundingClientRect().bottom+36);
  let current=settingsSections[0];
  for(const section of settingsSections){
    if(section.getBoundingClientRect().top<=edge)current=section;
  }
  if(window.scrollY>0&&window.innerHeight+window.scrollY>=document.documentElement.scrollHeight-2)current=settingsSections.at(-1);
  if(current)highlightSettingsSection(current.id);
}
settingsLinks.forEach(link=>link.addEventListener('click',event=>{
  event.preventDefault();
  const id=link.hash.slice(1);
  const section=document.getElementById(id);
  if(!section)return;
  settingsProgrammaticId=id;
  clearTimeout(settingsProgrammaticTimer);
  settingsProgrammaticTimer=setTimeout(()=>{settingsProgrammaticId=null;},700);
  highlightSettingsSection(id);
  const behavior=window.matchMedia('(prefers-reduced-motion: reduce)').matches?'instant':'smooth';
  if(id==='settings-runtime')window.scrollTo({top:0,behavior});
  else section.scrollIntoView({behavior,block:'start'});
  section.querySelector('h2')?.focus({preventScroll:true});
}));
document.addEventListener('scrollsnapchange',event=>{
  if(state.page!=='settings'||settingsProgrammaticId)return;
  const snapped=event.snapTargetBlock?.closest?.('#settings .settings-content > [id]');
  if(snapped)highlightSettingsSection(snapped.id);
});
window.addEventListener('scrollend',()=>{
  if(state.page!=='settings')return;
  clearTimeout(settingsProgrammaticTimer);
  settingsProgrammaticId=null;
  syncSettingsNavigation();
},{passive:true});
window.addEventListener('scroll',()=>{
  if(!settingsScrollFrame)settingsScrollFrame=requestAnimationFrame(syncSettingsNavigation);
},{passive:true});
window.addEventListener('resize',syncSettingsNavigation);

function updateStatus(status){
  state.status=status;
  const running=status.service==='running';
  $('service-pill').className=status.service;
  $('service-pill').querySelector('span').textContent=running?'API 运行中':status.service==='failed'?'API 启动失败':status.service==='starting'?'API 启动中':'API 已停止';
  $('api-state').textContent=running?'运行中':status.service==='failed'?'需要处理':'已停止';
  $('api-address').textContent=status.address||'仅监听本机地址';
  $('job-count').replaceChildren(node('span',String(status.running)),document.createTextNode(' '),node('em','执行中'));
  $('queue-count').textContent=`${status.queued} 个等待执行`;
  const activeCount=status.running+status.queued;
  $('nav-count').textContent=activeCount;
  $('nav-count').setAttribute('aria-label',`进行中任务 ${activeCount} 个`);
  $('error-count').textContent=status.failed?`${status.failed} 个失败／中断任务`:'任务状态正常';
  $('toggle-service').textContent=running?'停止 API':'启动 API';
  setAriaDisabled($('toggle-service'),activeCount>0||['starting','stopping'].includes(status.service));
  $('data-path').textContent=status.data_dir;
  $('browser-status').textContent=status.browser;
  if(status.address)$('base-url').textContent=status.address+'/v1';
  updateReadiness();
  if(status.last_error&&status.service==='failed')notice(status.last_error,true);
}

function selectedModel(){return state.models.find(model=>model.id===$('model').value)}
const kindLabels={image:'文生图 / 参考图编辑',t2v:'文字生成视频',omni:'文字 / 参考图视频',i2v:'首帧 / 首尾帧视频',r2v:'多参考图视频',extend:'续写已有视频',music_vocal:'带人声歌曲',music_instrumental:'纯器乐配乐'};
const pickerIds={type:'model-type',family:'model-family',kind:'model-kind',aspect_ratio:'model-ratio',spec:'model-spec'};
function pickerLabel(field,value){
  if(field==='type')return value==='image'?'图片':value==='video'?'视频':'音乐';
  if(field==='kind')return kindLabels[value]||value;
  if(field==='spec')return $('model-type').value==='video'?value+' 秒':value==='基础'?'基础（默认）':value;
  return value;
}
function syncModeTabs(activeType){
  document.querySelectorAll('.mode-switch [data-mode]').forEach(btn=>{
    const active=btn.dataset.mode===activeType;
    btn.classList.toggle('active',active);
    btn.setAttribute('aria-selected',active?'true':'false');
  });
}
function updatePromptMeta(){
  if($('prompt-count'))$('prompt-count').textContent=`${$('prompt').value.trim().length} 字 · ⌘/Ctrl + Enter 提交`;
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
  const type=result.selected.type;
  syncModeTabs(type);
  $('family-field').hidden=result.options.family.length<2;
  $('kind-field').hidden=result.options.kind.length<2;
  $('ratio-field').hidden=result.options.aspect_ratio.length<2;
  $('spec-label').textContent=type==='video'?'时长':type==='audio'?'生成模式':'清晰度';
  $('model').value=result.model?.id||'';
  modelInfo();
}
function modelInfo(){
  const model=selectedModel();
  if(!model)return;
  const c=model.capabilities;
  const isAudio=c.type==='audio';
  $('model-info').textContent=c.summary;
  $('video-field').hidden=!c.requires_video_id;
  $('music-fields').hidden=!isAudio;
  $('lyrics-field').hidden=!isAudio||c.supports_lyrics===false;
  $('reference-section').hidden=isAudio;
  $('prompt-label').textContent=isAudio?'风格与编曲描述':'描述';
  $('prompt').placeholder=isAudio
    ?'用英文写曲风、乐器、人声和情绪（如 Cheerful Mandarin children song, cute child vocals, ukulele, 110 BPM）'
    :'输入画面描述；使用参考图时，说明需要修改的内容。';
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
document.querySelectorAll('.mode-switch [data-mode]').forEach(btn=>{
  btn.onclick=()=>{
    $('model-type').value=btn.dataset.mode;
    const wanted=Object.fromEntries(Object.entries(pickerIds).map(([field,key])=>[field,$(key).value]));
    applyPicker(wanted);renderAccounts();saveDraft();clearComposeError();
  };
});
for(const id of Object.values(pickerIds))$(id).onchange=()=>{
  const wanted=Object.fromEntries(Object.entries(pickerIds).map(([field,key])=>[field,$(key).value]));
  applyPicker(wanted);renderAccounts();saveDraft();clearComposeError();
};

function studioRequest(){
  const model=selectedModel();
  const isAudio=model?.capabilities?.type==='audio';
  const rawSeconds=$('music-seconds')?.value?.trim();
  const seconds=rawSeconds?Math.max(15,Math.min(300,Math.round(Number(rawSeconds)))):null;
  const instrumental=isAudio&&model?.capabilities?.kind==='music_instrumental';
  return {
    model:model?.id||'',
    prompt:$('prompt').value.trim(),
    images:model?.capabilities.supports_images?[...state.images]:[],
    account_id:$('account').value||null,
    video_id:model?.capabilities.requires_video_id?$('video-id').value.trim()||null:null,
    lyrics:isAudio&&!instrumental?($('music-lyrics')?.value?.trim()||null):null,
    duration_seconds:isAudio&&Number.isFinite(seconds)?seconds:null,
    instrumental,
  };
}
function clearComposeError(){
  $('compose-error').hidden=true;
  for(const id of ['prompt','images','video-id','model-family']){
    const el=$(id);
    el.removeAttribute('aria-invalid');
    const tokens=(el.getAttribute('aria-describedby')||'').split(/\s+/).filter(t=>t&&t!=='compose-error');
    if(tokens.length)el.setAttribute('aria-describedby',tokens.join(' '));
    else el.removeAttribute('aria-describedby');
  }
}
function showComposeError(error){
  $('compose-error').textContent=error.message;$('compose-error').hidden=false;
  const el=$(error.field);
  el.setAttribute('aria-invalid','true');
  const tokens=(el.getAttribute('aria-describedby')||'').split(/\s+/).filter(Boolean);
  if(!tokens.includes('compose-error'))tokens.push('compose-error');
  el.setAttribute('aria-describedby',tokens.join(' '));
  el.focus();
}
function updateReadiness(){
  const running=state.status?.service==='running';
  const currentKind=selectedModel()?.capabilities?.type||$('model-type')?.value||'image';
  const isMusic=currentKind==='audio';
  const ready=state.accounts.filter(a=>accountReady(a,currentKind));
  const anyReady=state.accounts.filter(a=>isMusic?accountMusicReady(a):accountReady(a));
  const chosen=$('account').value;
  const chosenAccount=state.accounts.find(a=>a.id===chosen);
  let title='可以开始创作',detail=ready.length===1?`使用 ${ready[0].email}，作品自动保存到本机。`:`${ready.length} 个账号就绪，按优先级自动选择空闲账号。`,actionText='',actionKind='accounts',secondaryText='';
  if(!running){
    title=state.status?.service==='failed'?'本机 API 启动失败':'本机 API 尚未启动';
    detail=state.status?.last_error||'启动后即可连接浏览器并创作。';
    actionText='启动 API';actionKind='start';
  }
  else if(!anyReady.length){
    const tempCount=state.accounts.filter(a=>a.enabled&&isTempDisabled(a)).length;
    if(tempCount){
      title='账号正在临时冷却中';
      detail=`有 ${tempCount} 个账号处于冷却状态，可在 Google 账号页解除或等待恢复。`;
      actionText='管理账号';actionKind='accounts';
    }else if(isMusic){
      title='先连接 Flow Music 账号';
      detail='在默认浏览器打开 flowmusic.app 并登录，连接脚本会自动免码同步；保持该标签页打开即可创作音乐。';
      actionText='打开 Flow Music ↗';actionKind='open-music';
      secondaryText='账号页';
    }else{
      title='先连接 Flow 账号';
      detail='在默认浏览器打开 flow.google.com 并登录，连接脚本会自动免码同步；保持标签页打开即可创作。';
      actionText='打开 Flow ↗';actionKind='open-flow';
      secondaryText='账号页';
    }
  }
  else if(!ready.length){
    const kindName=currentKind==='video'?'视频':isMusic?'音乐':'图片';
    title=`没有开启${kindName}生成的就绪账号`;
    detail=`当前已连接的账号关闭了${kindName}生成开关，请在「Google 账号 → 管理」中开启或切换创作类型。`;
    actionText='调整账号开关';actionKind='accounts';
  }
  else if(chosen&&!ready.some(a=>a.id===chosen)){
    const kindName=currentKind==='video'?'视频':isMusic?'音乐':'图片';
    title='所选账号暂不可用';
    detail=chosenAccount&&isTempDisabled(chosenAccount)?`所选账号正在冷却中（${remainingCooldown(chosenAccount)}），请解除冷却或切换为自动分配。`:chosenAccount&&!accountSupportsKind(chosenAccount,currentKind)?`所选账号已关闭${kindName}生成开关，请开启或切换账号。`:`请重新连接该账号的 ${isMusic?'Flow Music':'Flow'} 标签页，或切换为自动分配。`;
    actionText='检查账号';actionKind='accounts';
  }
  if(state.loadingImages){title='正在读取参考图';detail='图片准备好后即可提交。';}
  $('readiness-title').textContent=title;$('readiness-detail').textContent=detail;
  $('readiness').classList.toggle('ready',running&&ready.length>0&&(!chosen||ready.some(a=>a.id===chosen)));
  $('readiness-action').hidden=!actionText;$('readiness-action').textContent=actionText;$('readiness-action').dataset.kind=actionKind;
  if($('readiness-secondary')){$('readiness-secondary').hidden=!secondaryText;$('readiness-secondary').textContent=secondaryText;}
  const cannotGenerate=state.submitting||state.loadingImages||!running||!ready.length||Boolean(chosen&&!ready.some(a=>a.id===chosen));
  setAriaDisabled($('generate'),cannotGenerate);
  $('generate').textContent=state.submitting?'正在提交…':isMusic?'生成歌曲':'生成';
}
$('readiness-action').onclick=()=>action($('readiness-action'),async()=>{
  const kind=$('readiness-action').dataset.kind;
  if(kind==='start'){await invoke('start_service');await refresh();}
  else if(kind==='open-flow'){await invoke('open_flow_page');}
  else if(kind==='open-music'){await invoke('open_flow_music_page');}
  else page('accounts');
});
if($('readiness-secondary'))$('readiness-secondary').onclick=()=>page('accounts');
if($('back-to-studio'))$('back-to-studio').onclick=()=>page('studio');

const DRAFT_KEY='flow-desktop-draft:v1';
let draft={};
try{draft=JSON.parse(localStorage.getItem(DRAFT_KEY)||'{}')||{};}catch{/* fresh draft */}
let draftDB=null,referenceRevision=0;
function saveDraft(){
  draft={
    model:$('model').value,
    prompt:$('prompt').value,
    account_id:$('account').value,
    video_id:$('video-id').value,
    music_seconds:$('music-seconds')?.value||'',
    music_lyrics:$('music-lyrics')?.value||'',
  };
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
  if($('music-seconds'))$('music-seconds').value=typeof draft.music_seconds==='string'?draft.music_seconds:'';
  if($('music-lyrics'))$('music-lyrics').value=typeof draft.music_lyrics==='string'?draft.music_lyrics:'';
  updatePromptMeta();
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
for(const id of ['prompt','video-id','music-seconds','music-lyrics']){
  if($(id))$(id).oninput=()=>{updatePromptMeta();saveDraft();clearComposeError();};
}
$('account').onchange=()=>{saveDraft();updateReadiness();};
for(const id of ['prompt','music-lyrics']){
  if($(id))$(id).onkeydown=event=>{
    if((event.ctrlKey||event.metaKey)&&event.key==='Enter'&&!event.isComposing){event.preventDefault();$('generate').click();}
  };
}
$('clear-draft').onclick=async()=>{
  if(($('prompt').value||state.images.length||$('video-id').value||$('music-lyrics')?.value||$('music-seconds')?.value)&&!confirm('清空当前描述、歌词、参考图和源视频？已生成的作品不会删除。'))return;
  $('prompt').value='';$('video-id').value='';
  if($('music-seconds'))$('music-seconds').value='';
  if($('music-lyrics'))$('music-lyrics').value='';
  state.images=[];$('images').value='';updatePromptMeta();clearComposeError();renderReferences();saveDraft();await saveReferences();
};
function renderReferences(){
  $('references').replaceChildren(...state.images.map((source,index)=>{
    const item=node('div',undefined,'reference-item'),image=node('img');image.src=source;image.alt=`参考图 ${index+1}`;
    const remove=button(`移除第 ${index+1} 张`,async()=>{state.images.splice(index,1);renderReferences();saveDraft();await saveReferences();modelInfo();clearComposeError();});
    item.append(image,remove);return item;
  }));
}
async function fillStudioFromRecord(promptText,modelId,videoId,request=null){
  if(($('prompt').value||state.images.length||$('video-id').value||$('music-lyrics')?.value)&&!confirm('用这条记录替换当前草稿？'))return;
  let model=state.models.find(m=>m.id===modelId);
  if(videoId)model=state.models.find(m=>m.capabilities.requires_video_id&&m.capabilities.family===model?.capabilities.family)||state.models.find(m=>m.capabilities.requires_video_id);
  if(model)applyPicker(FlowStudio.selection(model));
  $('prompt').value=promptText||'';$('video-id').value=videoId||'';
  if($('music-lyrics'))$('music-lyrics').value=request?.lyrics||'';
  if($('music-seconds'))$('music-seconds').value=request?.duration_seconds?String(request.duration_seconds):'';
  state.images=request?.images?[...request.images]:[];
  $('images').value='';
  $('account').value=request?.account_id&&state.accounts.some(a=>a.id===request.account_id)?request.account_id:'';
  updatePromptMeta();modelInfo();saveDraft();await saveReferences();clearComposeError();page('studio');$('prompt').focus();updateReadiness();
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

function explainErrorText(raw){
  const text=String(raw||'').trim();
  if(!text)return null;
  if(text.includes('PUBLIC_ERROR_MODEL_ACCESS_DENIED')||text.includes('ERROR_MODEL_ACCESS_DENIED')||text.includes('无该模型权限')){
    return {
      stageText:'失败：当前账号无该模型权限',
      title:'当前 Google 账号无该模型权限（PUBLIC_ERROR_MODEL_ACCESS_DENIED）',
      detail:'该账号的 Google 订阅等级不支持当前模型（例如 Veo 3.1 高清／专业版视频通常需要 Google AI Ultra 高阶订阅，普通 Pro 账号调用会被拒绝）。',
      advice:'处理建议：① 在「Google 账号 → 管理」中将 AI Ultra 账号设为「首选主力」；② 将普通账号取消勾选「允许生视频」（仅用于生图）；③ 或将模型切换为 Fast 规格重试。',
    };
  }
  if(text.includes('PUBLIC_ERROR_UNSAFE_GENERATION')||text.includes('PUBLIC_ERROR_SAFETY_FILTER')||text.includes('安全审核')){
    return {
      stageText:'失败：提示词或参考图触发安全审核拦截',
      title:'触发内容安全审核拦截（PUBLIC_ERROR_UNSAFE_GENERATION）',
      detail:'Google Flow 安全策略拦截了本次提交的描述词或参考图内容。',
      advice:'处理建议：调整提示词中的敏感或激烈措辞，或更换参考图后重试。',
    };
  }
  if(text.includes('PUBLIC_ERROR_PROMINENT_PEOPLE_FILTER_FAILED')||text.includes('PUBLIC_ERROR_PERSON_GENERATION')||text.includes('人物保护')){
    return {
      stageText:'失败：参考图或提示词触发人物保护策略',
      title:'触发特定人物／名人保护拦截（PUBLIC_ERROR_PROMINENT_PEOPLE_FILTER_FAILED）',
      detail:'参考图或提示词被识别为包含特定公众人物面部特征，被上游拦截。',
      advice:'处理建议：请更换不包含公众人物肖像的参考图，或修改人物描述。',
    };
  }
  if(text.includes('PUBLIC_ERROR_QUOTA_EXCEEDED')||text.includes('PUBLIC_ERROR_USER_QUOTA_REACHED')||text.includes('PUBLIC_ERROR_INSUFFICIENT_CREDITS')||text.includes('RESOURCE_EXHAUSTED')||text.includes('积分不足')){
    return {
      stageText:'失败：当前账号 AI 积分不足或配额耗尽',
      title:'账号 AI Credits 积分不足或达到配额上限',
      detail:'当前执行的 Google 账号剩余积分不足以支付本次生成，或已达到周期配额限制。',
      advice:'处理建议：自动分配模式下系统会自动冷却该账号并切换其他可用账号；也可在账号页核对余额或手动解除冷却。',
    };
  }
  if(text.includes('PUBLIC_ERROR_UNUSUAL_ACTIVITY')||text.includes('异常活动风控')){
    return {
      stageText:'失败：账号触发上游异常活动风控拦截',
      title:'当前账号触发 Google Flow 异常活动风控（PUBLIC_ERROR_UNUSUAL_ACTIVITY）',
      detail:'Google Flow 认为当前账号或会话短时间存在异常活动而拒绝本次提交。系统已对该账号自动施加渐进冷却（5→15→45→60 分钟）。',
      advice:`处理建议：请确保 Chrome 插件或连接脚本已更新至 v${APP_VERSION}+，等待冷却自动解除或使用其他可用账号分摊请求。`,
    };
  }
  if(text.includes('PUBLIC_ERROR_RATE_LIMIT_EXCEEDED')||text.includes('PUBLIC_ERROR_TOO_MANY_REQUESTS')||text.includes('触发上游限流')){
    return {
      stageText:'失败：请求过于频繁触发上游限流',
      title:'请求频率过高触发 Flow 限流',
      detail:'当前账号短时间内提交了过多请求，被 Google Flow 暂时限流。系统已对该账号自动施加渐进冷却。',
      advice:'处理建议：等待冷却恢复或配置多个账号分摊并发，也可在账号页随时手动解除冷却。',
    };
  }
  if(text.includes('Flow RPC 返回错误或缺少数据')||text.includes('NOT_FOUND')){
    return {
      stageText:'失败：上游拒绝或账号不支持该模型通道',
      title:'Flow 接口返回空数据或不支持当前模型键',
      detail:'Google Flow 未返回生成任务数据。常见原因：当前账号不支持 Relaxed 慢速通道、账号无该模型权限，或当前 Flow 项目会话已失效。',
      advice:'处理建议：应用已支持自动回退标准模型通道；可点击「换其他账号重试」或将主力高额度账号设为高优先级。',
    };
  }
  if(text.includes('Flow 浏览器操作失败')||text.includes('标签页已断开')||text.includes('未连接')){
    return {
      stageText:'失败：浏览器 Flow 标签页通信或网络异常',
      title:'浏览器 Flow 标签页执行请求失败',
      detail:'通过浏览器标签页调用 Flow 接口时失败（可能是网络／代理波动、Flow 页面未保持打开、登录过期或插件／脚本未加载）。',
      advice:'处理建议：确认浏览器中 Flow 页面保持打开且网络正常，在「Google 账号」页点击「同步」后再试。',
    };
  }
  if(text.includes('验证码')){
    return {
      stageText:'失败：获取 Google reCAPTCHA 验证码失败',
      title:'未能从浏览器获取 Flow 验证码',
      detail:'Flow 页面中的 reCAPTCHA Enterprise 校验未就绪或执行超时。',
      advice:`处理建议：刷新浏览器中的 Flow 页面，并确保已加载 v${APP_VERSION}+ 的 Chrome 插件或油猴脚本。`,
    };
  }
  return {
    stageText:text.length<=42?`失败：${text}`:'执行失败，请查看下方原因诊断',
    title:'任务未完成',
    detail:text,
    advice:'处理建议：请根据上方原因检查账号状态、网络连接或切换其他账号重试。',
  };
}

function taskCard(task){
  const card=node('article',undefined,'task-card');
  const header=node('header');
  const acct=state.accounts.find(a=>a.id===task.request?.account_id);
  const acctLabel=acct?` · ${acct.email}${acct.note?` (${acct.note})`:''}`:'';
  header.append(
    node('span',labels[task.state]||task.state,'state '+task.state),
    node('small',`${task.request.model}${acctLabel} · ${new Date(task.created_at).toLocaleString()}`)
  );
  const explained=task.error?explainErrorText(task.error):null;
  const stageText=(task.stage==='执行失败，请查看详情'&&explained)?explained.stageText:task.stage;
  card.append(header,expandablePrompt(task.request.prompt),node('div',stageText,'task-stage'));
  if(task.state==='running')card.append(node('small','执行中任务无法撤回；超时或失败后请先核对 Flow 结果，再决定是否重新提交。'));
  if(task.error&&explained){
    const box=node('div',undefined,'task-failure-box');
    const titleRow=node('div',undefined,'task-failure-title');
    titleRow.append(node('strong',explained.title));
    if(acct)titleRow.append(node('span',`执行账号：${acct.email}`,'task-failure-account'));
    box.append(titleRow,node('p',explained.detail,'task-failure-detail'),node('p',explained.advice,'task-failure-advice'));
    if(explained.detail!==task.error){
      box.append(node('small',`原始回执：${task.error}`,'task-failure-raw'));
    }
    const copyErrBtn=button('复制错误信息',()=>copy(task.error),'text-button');
    box.append(copyErrBtn);
    card.append(box);
  }
  for(const media of task.results||[]){
    if(media.kind==='audio'&&state.status?.address){
      const audioBox=node('div',undefined,'task-audio-box');
      if(media.title)audioBox.append(node('strong',media.title));
      const audio=node('audio');
      audio.controls=true;
      audio.preload='metadata';
      audio.src=state.status.address+'/media/'+media.filename;
      audio.setAttribute('aria-label',media.title?`生成歌曲：${media.title}`:'本机生成音乐');
      audioBox.append(audio);
      if(media.lyrics){
        const lyr=node('details',undefined,'prompt-details lyrics-details');
        lyr.append(node('summary','查看歌词'),node('pre',media.lyrics));
        audioBox.append(lyr);
      }
      card.append(audioBox);
    }
  }
  const actions=node('div',undefined,'task-actions');
  for(const media of task.results||[]){
    actions.append(button(media.kind==='image'?'打开图片 ↗':media.kind==='audio'?'打开音频 ↗':'打开视频 ↗',()=>invoke('open_media',{filename:media.filename})));
    if(media.kind==='audio'&&media.lyrics){
      actions.append(button('复制歌词',()=>copy(media.lyrics)));
    }
    if(media.kind==='video'&&media.media_id){
      actions.append(button('一键续写 ↗',()=>fillStudioFromRecord(task.request.prompt,task.request.model,media.media_id,{...task.request,images:[]})));
    }
  }
  if(task.request.prompt){
    actions.append(button(['failed','interrupted','cancelled'].includes(task.state)?'再次编辑后重试':'再次编辑',()=>editTask(task)));
  }
  if(task.state==='failed'&&acct){
    const otherReady=state.accounts.filter(a=>a.id!==acct.id&&accountReady(a)).sort((a,b)=>(b.priority||0)-(a.priority||0)||(b.credits||0)-(a.credits||0));
    if(otherReady.length){
      actions.append(button(`换 ${otherReady[0].email.split('@')[0]} 重试 ↗`,async()=>{
        const full=await invoke('task_detail',{id:task.id});
        await fillStudioFromRecord(full.request.prompt,full.request.model,full.request.video_id,{...full.request,account_id:otherReady[0].id});
        notice(`已切换为账号 ${otherReady[0].email}，可直接点击「开始创作」重试。`);
      },'primary'));
    }
    if(!isTempDisabled(acct)){
      actions.append(button('临时禁用该号 1h',async()=>{
        await invoke('temp_disable_account',{id:acct.id,minutes:60,reason:explained?.title||'任务执行失败手动冷却'});
        await refresh();
        notice(`已将 ${acct.email} 临时禁用 1 小时，期间自动分配将跳过该账号。`);
      }));
    }
  }
  if(task.state==='queued')actions.prepend(button('取消排队',async()=>{await invoke('cancel_task',{id:task.id});await refresh();notice('已取消排队，未提交到 Flow。');}));
  if(actions.children.length)card.append(actions);
  return card;
}

const tasksView=createListView({
  key:'tasks',
  hostId:'task-list',
  autoHidePager:true,
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
  searchText:task=>{
    const acct=state.accounts.find(a=>a.id===task.request?.account_id);
    const explained=task.error?explainErrorText(task.error):null;
    return [task.id,task.request?.prompt,task.request?.model,acct?.email,acct?.note,task.stage,task.error,explained?.title,explained?.detail,labels[task.state]].filter(Boolean).join(' ');
  },
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

function isTempDisabled(a){
  if(!a?.disabled_until)return false;
  const until=new Date(a.disabled_until).getTime();
  return Number.isFinite(until)&&until>Date.now();
}

function remainingCooldown(a){
  if(!isTempDisabled(a))return '';
  const mins=Math.max(1,Math.ceil((new Date(a.disabled_until).getTime()-Date.now())/60000));
  if(mins>=60){
    const hours=(mins/60).toFixed(mins%60===0?0:1);
    return `剩余约 ${hours} 小时`;
  }
  return `剩余 ${mins} 分钟`;
}

function accountSupportsKind(a,kind){
  if(kind==='video')return a.video_enabled!==false;
  if(kind==='audio')return a.music_enabled!==false;
  if(kind==='image')return a.image_enabled!==false;
  return a.image_enabled!==false||a.video_enabled!==false||a.music_enabled!==false;
}

function accountFlowOnline(a){
  const connection=state.connections.find(c=>c.account_id===a.id);
  return !a.browser_bridge||Boolean(connection?.connected&&connection.state==='ready');
}

function accountMusicOnline(a){
  const connection=state.musicConnections.find(c=>c.account_id===a.id);
  return Boolean(connection?.connected&&connection.state==='ready');
}

function accountMusicReady(a){
  return a.enabled&&!isTempDisabled(a)&&a.music_enabled!==false&&Boolean(a.music_last_synced)&&accountMusicOnline(a);
}

function accountReady(a,kind=null){
  if(kind==='audio')return accountMusicReady(a);
  if(kind===null){
    const flowOk=a.enabled&&!isTempDisabled(a)&&Boolean(a.last_synced)&&accountFlowOnline(a);
    return flowOk||accountMusicReady(a);
  }
  return a.enabled&&!isTempDisabled(a)&&accountSupportsKind(a,kind)&&Boolean(a.last_synced)&&accountFlowOnline(a);
}

const PRIORITY_OPTIONS=[
  {value:100,label:'首选主力 (100)',short:'首选主力'},
  {value:50,label:'高优先 (50)',short:'高优先'},
  {value:0,label:'普通 (0)',short:'普通'},
  {value:-20,label:'低优先 (-20)',short:'低优先'},
  {value:-50,label:'仅作备用 (-50)',short:'备用'},
];

function priorityBadgeText(p){
  const val=Number(p||0);
  const found=PRIORITY_OPTIONS.find(o=>o.value===val);
  if(found)return found.short;
  return val>0?`优先 +${val}`:`优先 ${val}`;
}

function openAccountDialog(account){
  const previousFocus=document.activeElement;
  document.getElementById('account-dialog')?.remove();
  const dialog=node('dialog',undefined,'profile-dialog account-dialog');
  dialog.id='account-dialog';
  dialog.setAttribute('aria-labelledby','account-dialog-title');

  const header=node('div',undefined,'section-heading');
  const headingWrap=node('div');
  const title=node('h2','管理账号');
  title.id='account-dialog-title';
  title.tabIndex=-1;
  const subParts=[account.email];
  if(account.credits!==null&&account.credits!==undefined){
    subParts.push(`Flow ${Number(account.credits).toLocaleString()} credits`);
  }
  if(account.music_credits!==null&&account.music_credits!==undefined){
    subParts.push(`Music ${Number(account.music_credits).toLocaleString()} credits`);
  }
  headingWrap.append(title,node('p',subParts.join(' · '),'hint'));
  header.append(headingWrap,button('关闭',()=>dialog.close(),'quiet'));

  const form=node('form',undefined,'dialog-body account-dialog-form');
  form.id='account-edit-form';

  const enabledLabel=node('label',undefined,'migration-checkbox');
  const enabledCheck=node('input');
  enabledCheck.type='checkbox';
  enabledCheck.id='acct-edit-enabled';
  enabledCheck.name='enabled';
  enabledCheck.checked=Boolean(account.enabled);
  enabledLabel.append(enabledCheck,node('span','启用此账号参与生成'));
  form.append(enabledLabel);

  const prioLabel=node('label','调度优先级');
  const prioSelect=node('select');
  prioSelect.id='acct-edit-priority';
  prioSelect.name='priority';
  const currentPrio=Number(account.priority||0);
  const options=PRIORITY_OPTIONS.some(o=>o.value===currentPrio)?PRIORITY_OPTIONS:[{value:currentPrio,label:`自定义 (${currentPrio})`,short:`${currentPrio}`},...PRIORITY_OPTIONS];
  for(const item of options){
    const opt=node('option',item.label);
    opt.value=String(item.value);
    opt.selected=item.value===currentPrio;
    prioSelect.append(opt);
  }
  prioLabel.append(prioSelect);
  form.append(prioLabel);

  const capsWrap=node('div',undefined,'account-dialog-field');
  capsWrap.append(node('span','生成能力','account-dialog-label'));
  const capsRow=node('div',undefined,'account-dialog-caps');
  const imgToggle=node('label',undefined,'cap-toggle');
  const imgCheck=node('input');
  imgCheck.type='checkbox';
  imgCheck.id='acct-edit-image';
  imgCheck.name='image_enabled';
  imgCheck.checked=account.image_enabled!==false;
  imgToggle.append(imgCheck,node('span','允许生图'));
  const vidToggle=node('label',undefined,'cap-toggle');
  const vidCheck=node('input');
  vidCheck.type='checkbox';
  vidCheck.id='acct-edit-video';
  vidCheck.name='video_enabled';
  vidCheck.checked=account.video_enabled!==false;
  vidToggle.append(vidCheck,node('span','允许生视频'));
  const musToggle=node('label',undefined,'cap-toggle');
  const musCheck=node('input');
  musCheck.type='checkbox';
  musCheck.id='acct-edit-music';
  musCheck.name='music_enabled';
  musCheck.checked=account.music_enabled!==false;
  musToggle.append(musCheck,node('span','允许生音乐'));
  capsRow.append(imgToggle,vidToggle,musToggle);
  capsWrap.append(capsRow);
  form.append(capsWrap);

  const noteLabel=node('label','备注');
  const noteInput=node('input');
  noteInput.type='text';
  noteInput.id='acct-edit-note';
  noteInput.name='note';
  noteInput.maxLength=500;
  noteInput.value=account.note||'';
  noteInput.placeholder='例如 AI Ultra 主力、仅生图，留空清除';
  noteLabel.append(noteInput);
  form.append(noteLabel);

  let cooldownSelect=null;
  if(isTempDisabled(account)){
    const coolBox=node('div',undefined,'account-dialog-cooldown');
    const coolText=node('div');
    coolText.append(
      node('strong',`当前处于临时冷却（${remainingCooldown(account)}）`),
      node('p',`${account.disabled_reason||'手动临时禁用'} · 至 ${new Date(account.disabled_until).toLocaleTimeString()} 自动恢复`,'hint')
    );
    coolBox.append(coolText,button('立即解除',async()=>{
      await invoke('temp_disable_account',{id:account.id,minutes:0,reason:null});
      await refresh();
      dialog.close();
      notice(`已解除 ${account.email} 的临时冷却。`);
    },'primary'));
    form.append(coolBox);
  }else{
    const coolLabel=node('label','临时冷却');
    cooldownSelect=node('select');
    cooldownSelect.id='acct-edit-cooldown';
    cooldownSelect.name='cooldown';
    for(const [val,label] of [['0','不冷却（正常分配）'],['15','暂停 15 分钟'],['60','暂停 1 小时'],['360','暂停 6 小时'],['1440','暂停 24 小时']]){
      const opt=node('option',label);
      opt.value=val;
      cooldownSelect.append(opt);
    }
    coolLabel.append(cooldownSelect);
    form.append(coolLabel);
  }

  const maintenance=node('div',undefined,'account-dialog-maintenance');
  maintenance.append(node('span','维护操作','account-dialog-label'));
  const maintActions=node('div',undefined,'actions');
  maintActions.append(
    button('打开 Flow ↗',()=>invoke('open_flow_page')),
    button('打开 Flow Music ↗',()=>invoke('open_flow_music_page')),
    button('同步 Flow',async()=>{
      await invoke('sync_account',{id:account.id});
      await refresh();
      notice(`${account.email} Flow 余额已同步。`);
    }),
    button('同步 Music',async()=>{
      await invoke('sync_music_account',{id:account.id});
      await refresh();
      notice(`${account.email} Flow Music 积分已同步。`);
    })
  );
  if(account.project_id){
    maintActions.append(button('重置 Flow 项目',async()=>{
      await invoke('save_account',{id:account.id,email:account.email,enabled:account.enabled,resetProject:true});
      await refresh();
      notice('已清空旧项目 ID，下次生成将自动创建新 Flow 项目。');
    }));
  }
  if(account.last_error){
    maintActions.append(button('清除报错',async()=>{
      await invoke('save_account',{id:account.id,email:account.email,enabled:account.enabled,clearError:true});
      await refresh();
      notice('已清除该账号的错误记录。');
    }));
  }
  maintActions.append(button('删除账号',async()=>{
    if(!confirm(`删除 ${account.email} 的应用登记？浏览器中的 Google 登录态不受影响。`))return;
    await invoke('delete_account',{id:account.id});
    await refresh();
    dialog.close();
    notice(`已删除 ${account.email}。`);
  },'danger'));
  maintenance.append(maintActions);
  form.append(maintenance);

  const footer=node('div',undefined,'actions');
  const saveBtn=node('button','保存','primary');
  saveBtn.type='submit';
  saveBtn.setAttribute('form','account-edit-form');
  footer.append(saveBtn,button('取消',()=>dialog.close(),'secondary'));

  form.onsubmit=event=>{
    event.preventDefault();
    action(saveBtn,async()=>{
      await invoke('save_account',{
        id:account.id,
        email:account.email,
        enabled:enabledCheck.checked,
        priority:Number(prioSelect.value),
        imageEnabled:imgCheck.checked,
        videoEnabled:vidCheck.checked,
        musicEnabled:musCheck.checked,
        note:noteInput.value,
      });
      const mins=cooldownSelect?Number(cooldownSelect.value):0;
      if(mins>0){
        await invoke('temp_disable_account',{id:account.id,minutes:mins,reason:null});
      }
      await refresh();
      dialog.close();
      notice(`${account.email} 配置已保存。`);
    });
  };

  dialog.append(header,form,footer);
  dialog.addEventListener('close',()=>{dialog.remove();previousFocus?.focus?.();},{once:true});
  document.body.append(dialog);
  dialog.showModal();
  title.focus({preventScroll:true});
}

function accountCard(account){
  const row=node('article',undefined,'account-card');
  const info=node('div',undefined,'account-info');
  const connection=state.connections.find(c=>c.account_id===account.id);
  const musicConn=state.musicConnections.find(c=>c.account_id===account.id);
  const flowOnline=accountFlowOnline(account)&&Boolean(account.last_synced);
  const musicOnline=accountMusicOnline(account)&&Boolean(account.music_last_synced);
  const tempDisabled=isTempDisabled(account);
  const ready=accountReady(account);

  const titleRow=node('div',undefined,'account-title-row');
  titleRow.append(node('strong',account.email));
  const statusBadge=node(
    'span',
    !account.enabled?'已停用':tempDisabled?`冷却中 · ${remainingCooldown(account)}`:ready?'已就绪':'未连接',
    'state '+(!account.enabled?'cancelled':tempDisabled?'running':ready?'succeeded':'failed')
  );
  titleRow.append(statusBadge);
  if(account.credits!==null&&account.credits!==undefined){
    titleRow.append(node('span',`Flow ${Number(account.credits).toLocaleString()}`,'tag'));
  }
  if(account.music_credits!==null&&account.music_credits!==undefined){
    titleRow.append(node('span',`Music ${Number(account.music_credits).toLocaleString()}`,'tag'));
  }
  if(account.note){
    titleRow.append(node('span',account.note,'tag tag-note'));
  }
  info.append(titleRow);

  const metaParts=[];
  const prio=Number(account.priority||0);
  if(prio!==0)metaParts.push(priorityBadgeText(prio));
  const disabledCaps=[];
  if(account.image_enabled===false)disabledCaps.push('图片');
  if(account.video_enabled===false)disabledCaps.push('视频');
  if(account.music_enabled===false)disabledCaps.push('音乐');
  if(disabledCaps.length)metaParts.push(`已关${disabledCaps.join('／')}`);

  if(tempDisabled&&account.disabled_reason){
    metaParts.push(`${account.disabled_reason}（至 ${new Date(account.disabled_until).toLocaleTimeString()} 恢复）`);
  }else{
    const connParts=[];
    if(flowOnline)connParts.push('Flow 已连接');
    else if(connection?.connected&&connection.message)connParts.push(`Flow：${connection.message}`);
    if(musicOnline)connParts.push('Music 已连接');
    else if(musicConn?.connected&&musicConn.message)connParts.push(`Music：${musicConn.message}`);
    if(connParts.length){
      metaParts.push(connParts.join(' · '));
    }else if(!account.browser_bridge){
      metaParts.push('旧版账号 · 在默认浏览器打开 Flow 重新直连');
    }else{
      metaParts.push('未连接 · 打开 Flow 或 Flow Music 页面即可自动同步');
    }
    const latestSync=account.last_synced&&account.music_last_synced
      ?(account.last_synced>account.music_last_synced?account.last_synced:account.music_last_synced)
      :(account.last_synced||account.music_last_synced);
    if(latestSync&&(flowOnline||musicOnline)){
      metaParts.push(`同步于 ${new Date(latestSync).toLocaleTimeString()}`);
    }
  }
  info.append(node('p',metaParts.join(' · '),'account-meta'));

  if(account.last_error){
    const errInfo=explainErrorText(account.last_error);
    const errBox=node('div',undefined,'account-error-box');
    const errText=node('span',`最近报错${account.consecutive_failures>1?`（连续 ${account.consecutive_failures} 次）`:''}：${errInfo?errInfo.title:account.last_error}`);
    const clearBtn=button('清除',async()=>{
      await invoke('save_account',{id:account.id,email:account.email,enabled:account.enabled,clearError:true});
      await refresh();
      notice('已清除该账号的错误记录。');
    },'text-button');
    errBox.append(errText,clearBtn);
    info.append(errBox);
  }

  const actions=node('div',undefined,'actions');
  if(tempDisabled){
    actions.append(button('解除冷却',async()=>{
      await invoke('temp_disable_account',{id:account.id,minutes:0,reason:null});
      await refresh();
      notice(`已解除 ${account.email} 的临时冷却。`);
    },'primary'));
  }else if(!account.enabled){
    actions.append(button('启用',async()=>{
      await invoke('save_account',{id:account.id,email:account.email,enabled:true});
      await refresh();
      notice(`已启用 ${account.email}。`);
    }));
  }else if(flowOnline||musicOnline){
    actions.append(button('同步',async()=>{
      const errs=[];
      if(flowOnline){try{await invoke('sync_account',{id:account.id});}catch(e){errs.push('Flow：'+(e?.message||e));}}
      if(musicOnline){try{await invoke('sync_music_account',{id:account.id});}catch(e){errs.push('Music：'+(e?.message||e));}}
      await refresh();
      if(errs.length)notice(errs.join('；'),true);
      else notice(`${account.email} 最新状态与积分已同步。`);
    }));
  }else{
    actions.append(button('去连接 ↗',()=>invoke('open_flow_page')));
  }
  actions.append(button('管理',()=>openAccountDialog(account)));

  row.append(info,actions);
  return row;
}

const accountsView=createListView({
  key:'accounts',
  hostId:'account-list',
  autoHideToolbar:4,
  autoHidePager:true,
  searchPlaceholder:'搜索账号邮箱、备注或报错原因…',
  filterLabel:'账号状态',
  filters:[
    {value:'all',label:'全部账号'},
    {value:'ready',label:'已就绪'},
    {value:'temp_disabled',label:'冷却中'},
    {value:'unsynced',label:'未连接'},
    {value:'disabled',label:'已停用'},
  ],
  matchFilter:(a,filter)=>{
    if(filter==='ready')return accountReady(a);
    if(filter==='temp_disabled')return a.enabled&&isTempDisabled(a);
    if(filter==='unsynced')return a.enabled&&!isTempDisabled(a)&&!accountReady(a);
    if(filter==='disabled')return !a.enabled;
    return true;
  },
  searchText:a=>[a.email,a.note,a.last_error,a.disabled_reason,a.project_id].filter(Boolean).join(' '),
  sorts:[
    {value:'priority',label:'优先级',get:a=>Number(a.priority||0)},
    {value:'credits',label:'积分余额',get:a=>a.credits??-1},
    {value:'last_synced',label:'同步时间',get:a=>a.last_synced||''},
    {value:'email',label:'邮箱',get:a=>a.email||''},
  ],
  defaultOrder:'desc',
  emptyText:'还没有已连接的账号。推荐点击上方「安装 Chrome 插件 ↗」（或安装油猴脚本），再打开 Flow 或 Flow Music 页面登录即可自动直连。',
  noMatchText:'没有匹配当前筛选条件的 Google 账号。',
  revisionKey:items=>[items,state.connections,state.musicConnections],
  renderItem:accountCard,
});

function renderAccounts(){
  const select=$('account');
  const previous=select.value||draft.account_id||'';
  const currentKind=selectedModel()?.capabilities?.type||$('model-type')?.value||'image';
  const signature=JSON.stringify(state.accounts.map(a=>[a.id,a.email,a.priority,a.credits,a.music_credits,a.note,accountReady(a,currentKind),isTempDisabled(a)]));
  if(select.dataset.signature!==signature){
    select.dataset.signature=signature;
    const automatic=node('option','自动分配（按优先级与空闲状态）');automatic.value='';
    const sorted=[...state.accounts].sort((a,b)=>(b.priority||0)-(a.priority||0)||(b.credits||0)-(a.credits||0));
    select.replaceChildren(automatic,...sorted.map(account=>{
      const kindLabel=currentKind==='video'?'视频':currentKind==='audio'?'音乐':'生图';
      const statusSuffix=accountReady(account,currentKind)?'':isTempDisabled(account)?' · 冷却中':!account.enabled?' · 已停用':!accountSupportsKind(account,currentKind)?` · 已关${kindLabel}`:' · 未连接';
      const creditsVal=currentKind==='audio'?account.music_credits:account.credits;
      const creditText=creditsVal!=null?` (${Number(creditsVal).toLocaleString()} credits)`:'';
      const prioText=account.priority?` [${priorityBadgeText(account.priority)}]`:'';
      const option=node('option',`${account.email}${prioText}${creditText}${statusSuffix}`);
      option.value=account.id;
      return option;
    }));
    select.value=state.accounts.some(a=>a.id===previous)?previous:'';
  }
  const anyLive=state.connections.some(c=>c.connected&&c.state==='ready')||state.musicConnections.some(c=>c.connected&&c.state==='ready');
  if($('accounts-chrome-tip'))$('accounts-chrome-tip').hidden=anyLive;
  if($('sync-all-accounts'))$('sync-all-accounts').hidden=state.accounts.length===0;
  accountsView.update(state.accounts);updateReadiness();
  $('account').closest('label').hidden=state.accounts.length<2;
}

function openImagePreview(media){
  if(!state.status?.address)return;
  const previousFocus=document.activeElement;
  document.getElementById('image-lightbox')?.remove();
  const dialog=node('dialog',undefined,'profile-dialog lightbox-dialog');
  dialog.id='image-lightbox';
  dialog.setAttribute('aria-labelledby','lightbox-title');
  const header=node('div',undefined,'section-heading');
  const wrap=node('div');
  const title=node('h2','图片预览');
  title.id='lightbox-title';
  title.tabIndex=-1;
  wrap.append(title,node('p',`${media.model||'图片作品'} · ${(media.size/1024/1024).toFixed(2)} MB · ${new Date(media.created_at).toLocaleString()}`,'hint'));
  header.append(wrap,button('关闭',()=>dialog.close(),'quiet'));
  const body=node('div',undefined,'dialog-body');
  const stage=node('div',undefined,'lightbox-stage');
  const img=node('img');
  img.src=state.status.address+'/media/'+media.filename;
  img.alt=media.prompt||'本机生成作品';
  stage.append(img);
  body.append(stage);
  if(media.prompt)body.append(node('p',media.prompt,'hint'));
  const footer=node('div',undefined,'actions');
  footer.append(
    button('打开原文件 ↗',()=>invoke('open_media',{filename:media.filename}),'primary'),
    button('用作参考图',async()=>{
      const source=await invoke('media_reference',{filename:media.filename});
      if(!selectedModel()?.capabilities.supports_images){
        const model=state.models.find(m=>m.id==='gemini-3.1-flash-lite-image-square');if(model)applyPicker(FlowStudio.selection(model));
      }
      const max=selectedModel()?.capabilities.max_images||0;
      if(state.images.length>=max)throw Error(`当前模型最多支持 ${max} 张参考图，请先移除不需要的图片。`);
      state.images.push(source);renderReferences();saveDraft();await saveReferences();dialog.close();page('studio');notice('已加入参考图，填写修改要求后即可生成。');
    }),
    button('关闭',()=>dialog.close(),'secondary')
  );
  dialog.append(header,body,footer);
  dialog.addEventListener('close',()=>{dialog.remove();previousFocus?.focus?.();},{once:true});
  document.body.append(dialog);
  dialog.showModal();
  title.focus({preventScroll:true});
}

function mediaCard(media){
  const card=node('article',undefined,'media-card');
  if(media.kind==='audio'){
    const cover=node('div',undefined,'audio-cover');
    const icon=node('div',undefined,'audio-icon');
    icon.innerHTML='<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M9 18V5l12-2v13"/><circle cx="6" cy="18" r="3"/><circle cx="18" cy="16" r="3"/></svg>';
    const meta=node('div',undefined,'audio-meta');
    meta.append(node('strong',media.title||'Flow Music 歌曲'),node('small',`${media.model||'Flow Music'} · ${(media.size/1024/1024).toFixed(2)} MB`));
    cover.append(icon,meta);
    card.append(cover);
    const info=node('div');
    if(state.status?.address){
      const player=node('audio',undefined,'audio-player');
      player.controls=true;
      player.preload='metadata';
      player.src=state.status.address+'/media/'+media.filename;
      player.setAttribute('aria-label',media.title?`播放歌曲：${media.title}`:'播放生成音频');
      info.append(player);
    }else{
      info.append(node('p','启动本机 API 可直接试听；仍可打开本机文件。','hint'));
    }
    info.append(node('small',`音频 · ${new Date(media.created_at).toLocaleString()}`));
    if(media.prompt)info.append(expandablePrompt(media.prompt));
    if(media.lyrics){
      const lyr=node('details',undefined,'prompt-details lyrics-details');
      lyr.append(node('summary','查看歌词'),node('pre',media.lyrics));
      info.append(lyr);
    }
    const actions=node('div',undefined,'actions');
    actions.append(
      button('打开文件 ↗',()=>invoke('open_media',{filename:media.filename})),
      button('复制链接',async()=>{
        if(!state.status?.address)throw Error('请先启动 API');
        await copy(state.status.address+'/media/'+media.filename);
      })
    );
    if(media.lyrics)actions.append(button('复制歌词',()=>copy(media.lyrics)));
    if(media.prompt||media.model){
      actions.append(button('填入工作台',()=>fillStudioFromRecord(media.prompt,media.model,null,{lyrics:media.lyrics})));
    }
    info.append(actions);
    card.append(info);
    return card;
  }
  const preview=node(media.kind==='image'?'img':'video');
  if(media.kind==='video'){
    preview.controls=true;
    preview.preload='metadata';
    preview.setAttribute('aria-label',media.prompt?`生成视频：${media.prompt.slice(0,60)}`:'本机生成视频');
  }else{
    preview.alt=media.prompt||'本机生成作品';
    preview.loading='lazy';
    preview.title='点击放大预览';
    preview.onclick=()=>openImagePreview(media);
  }
  if(state.status?.address){
    preview.src=state.status.address+'/media/'+media.filename;
  }else{
    preview.hidden=true;
  }
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
  autoHidePager:true,
  bodyClass:'media-grid-items',
  searchPlaceholder:'搜索作品提示词、歌名、歌词、模型或文件名…',
  filterLabel:'作品类型',
  filters:[
    {value:'all',label:'全部类型'},
    {value:'image',label:'图片'},
    {value:'video',label:'视频'},
    {value:'audio',label:'音乐'},
  ],
  matchFilter:(m,filter)=>m.kind===filter,
  searchText:m=>[m.title,m.lyrics,m.prompt,m.model,m.filename,m.media_id,m.kind].filter(Boolean).join(' '),
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
  if($('auto-update'))$('auto-update').checked=Boolean(state.settings.auto_update);
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
  const [accounts,tasks,media,status,connections,musicConnections]=await Promise.all([
    invoke('accounts'),
    invoke('tasks'),
    invoke('library'),
    invoke('status'),
    invoke('browser_connections'),
    invoke('music_connections').catch(()=>[]),
  ]);
  state.connections=connections;
  state.musicConnections=musicConnections;
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
$('open-flow').onclick=()=>action($('open-flow'),()=>invoke('open_flow_page'));
if($('open-flow-music'))$('open-flow-music').onclick=()=>action($('open-flow-music'),()=>invoke('open_flow_music_page'));
if($('sync-all-accounts'))$('sync-all-accounts').onclick=()=>action($('sync-all-accounts'),async()=>{
  const targets=state.accounts.filter(a=>a.enabled);
  if(!targets.length)throw Error('没有已启用的 Google 账号');
  let ok=0,failed=[];
  for(let i=0;i<targets.length;i++){
    const a=targets[i];
    const hasFlow=accountFlowOnline(a);
    const hasMusic=accountMusicOnline(a);
    if(!hasFlow&&!hasMusic){failed.push(`${a.email}（浏览器未连接）`);continue;}
    notice(`正在同步账号 (${i+1}/${targets.length})：${a.email}…`);
    let syncedAny=false;
    if(hasFlow){
      try{await invoke('sync_account',{id:a.id});syncedAny=true;}
      catch(err){failed.push(`${a.email} Flow（${err?.message||err}）`);}
    }
    if(hasMusic){
      try{await invoke('sync_music_account',{id:a.id});syncedAny=true;}
      catch(err){failed.push(`${a.email} Music（${err?.message||err}）`);}
    }
    if(syncedAny)ok++;
  }
  await refresh();
  if(failed.length){
    notice(`同步完成：成功 ${ok} 个，未同步 ${failed.length} 项：${failed.join('；')}`,true);
  }else{
    notice(`全部 ${ok} 个已连接账号已同步最新状态与积分余额。`);
  }
});
if($('open-chrome-extension'))$('open-chrome-extension').onclick=()=>action($('open-chrome-extension'),async()=>{
  const dir=await invoke('open_chrome_extension_dir');
  if(dir){
    try{await navigator.clipboard.writeText(dir);}catch{/* clipboard unavailable in non-secure fixture */}
  }
  notice(`已打开 Chrome 插件目录并复制路径（v${APP_VERSION}）：在浏览器打开 chrome://extensions 开启开发者模式 → 加载已解压的扩展程序，粘贴路径即可（无需油猴）。`);
});
$('open-userscript').onclick=()=>action($('open-userscript'),async()=>{
  await invoke('open_userscript_url');
  notice(`已在浏览器中打开油猴脚本（v${APP_VERSION}）安装页面。Chrome 138+ 请确认 Tampermonkey 已开启「允许运行用户脚本」（或改用免配置的 Chrome 插件）。`);
});

async function addReferenceFiles(fileList){
  const files=[...fileList].filter(Boolean);
  if(!files.length)return;
  if(!selectedModel()?.capabilities?.supports_images){
    notice('当前创作模式不使用参考图片。',true);
    return;
  }
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
    notice(`已添加 ${images.length} 张参考图。`);
  }catch(error){notice(error,true);}
  finally{$('images').value='';state.loadingImages=false;modelInfo();updateReadiness();}
}
$('images').onchange=()=>addReferenceFiles($('images').files);
const dropzone=$('reference-dropzone');
if(dropzone){
  dropzone.addEventListener('dragover',e=>{e.preventDefault();dropzone.classList.add('dragover');});
  dropzone.addEventListener('dragleave',()=>dropzone.classList.remove('dragover'));
  dropzone.addEventListener('drop',e=>{
    e.preventDefault();
    dropzone.classList.remove('dragover');
    if(e.dataTransfer?.files?.length)addReferenceFiles(e.dataTransfer.files);
  });
}
document.addEventListener('paste',e=>{
  if(state.page!=='studio')return;
  const items=[...(e.clipboardData?.items||[])].filter(item=>item.kind==='file'&&item.type.startsWith('image/'));
  if(!items.length)return;
  const files=items.map(item=>item.getAsFile()).filter(Boolean);
  if(files.length){e.preventDefault();addReferenceFiles(files);}
});
if($('toggle-api-key'))$('toggle-api-key').onclick=()=>{
  const input=$('setting-api-key');
  const shown=input.type==='text';
  input.type=shown?'password':'text';
  $('toggle-api-key').textContent=shown?'显示':'隐藏';
  $('toggle-api-key').setAttribute('aria-pressed',shown?'false':'true');
};

$('generate').onclick=async()=>{
  if(state.submitting)return;
  if($('generate').getAttribute('aria-disabled')==='true'){
    notice($('readiness-detail').textContent,true);
    if(!$('readiness-action').hidden)$('readiness-action').focus();
    return;
  }
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
    settings.browser_executable=state.settings?.browser_executable||'';
    settings.auto_update=Boolean($('auto-update')?.checked??state.settings?.auto_update??true);
    for(const name of ['port','max_concurrent','image_timeout_seconds','video_timeout_seconds','music_timeout_seconds'])settings[name]=Number(settings[name]);
    const portChanged=state.settings&&state.settings.port!==settings.port;
    await invoke('save_settings',{settings});
    state.settings=settings;
    $('base-url').textContent=`http://127.0.0.1:${settings.port}/v1`;
    await loadMagpie();
    await refresh();
    notice(portChanged&&state.status?.service==='running'?'设置已保存并已在新端口重启 API 生效。':'设置已保存并立即生效。');
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
  const box=$('autostart');
  if(box.dataset.busy==='1'){box.checked=!box.checked;return;}
  const want=box.checked;
  box.dataset.busy='1';
  setAriaDisabled(box,true);
  try{
    const actual=await invoke('set_autostart',{enabled:want});
    box.checked=actual;
    notice(actual?'已打开登录后自动启动':'已关闭登录后自动启动');
  }catch(error){
    try{box.checked=await invoke('autostart_enabled');}catch{box.checked=false;}
    notice(error,true);
  }finally{
    delete box.dataset.busy;
    setAriaDisabled(box,false);
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
let latestUpdateStatus=null;

function syncVersionLabels(version){
  if(!version)return;
  APP_VERSION=version;
  if($('aside-version'))$('aside-version').textContent=`v${version}`;
  if($('settings-current-version'))$('settings-current-version').textContent=`v${version}`;
  if($('footer-version'))$('footer-version').textContent=`Flow2API Desktop v${version} · Rust + Tauri`;
  const dialogVer=document.getElementById('dialog-current-version');
  if(dialogVer)dialogVer.textContent=`当前运行版本：v${version}`;
}

function renderUpdateStatus(status){
  if(!status)return;
  latestUpdateStatus=status;
  if(status.current_version)syncVersionLabels(status.current_version);
  if(typeof status.auto_update==='boolean'&&$('auto-update'))$('auto-update').checked=status.auto_update;
  if(state.settings&&typeof status.auto_update==='boolean')state.settings.auto_update=status.auto_update;
  const hasAvailable=Boolean(status.available_version);
  if(hasAvailable){
    pendingUpdate={version:status.available_version,current_version:status.current_version||APP_VERSION,body:status.notes||'',downloaded:Boolean(status.downloaded)};
  }else if(status.phase==='current'){
    pendingUpdate=null;
  }
  const checked=status.checked_at?` 上次检查：${new Date(status.checked_at*1000).toLocaleString()}。`:'';
  const text=(status.message||'从公开的 GitHub Release 检查更新，无需登录。')+checked;
  $('update-status').textContent=text;
  const dialogStatus=document.getElementById('dialog-update-status');
  if(dialogStatus)dialogStatus.textContent=text;
  const dialogNotes=document.getElementById('dialog-update-notes');
  if(dialogNotes){
    if(status.notes){
      dialogNotes.textContent=status.notes;
      dialogNotes.style.display='block';
    }else if(!hasAvailable){
      dialogNotes.style.display='none';
    }
  }
  const installLabel=status.phase==='installing'
    ?`正在安装 v${status.available_version||''}…`
    :status.downloaded
      ?`立即安装 v${status.available_version} 并重启`
      :`下载并安装 v${status.available_version||''}`;
  const installBusy=status.phase==='installing'||status.phase==='downloading';
  $('install-update').hidden=!hasAvailable;
  $('install-update').textContent=installLabel;
  setAriaDisabled($('install-update'),installBusy);
  const dialogInstall=document.getElementById('dialog-install-btn');
  if(dialogInstall){
    dialogInstall.style.display=hasAvailable?'inline-block':'none';
    dialogInstall.textContent=installLabel;
    setAriaDisabled(dialogInstall,installBusy);
  }
  if($('update-badge')){
    $('update-badge').textContent=({
      error:'更新失败',
      checking:'检查中',
      current:'已是最新',
      available:`可升级 v${status.available_version||''}`,
      downloading:'后台下载中',
      ready:`已就绪 v${status.available_version||''}`,
      waiting:'等待空闲安装',
      installing:'安装中',
    })[status.phase]||'检查更新';
  }
  const navUpdateLabel=status.downloaded&&status.available_version
    ?`安装 v${status.available_version} ↗`
    :status.available_version
      ?`新版本 v${status.available_version} ↗`
      :'检查更新 ↗';
  if($('header-check-update')){
    $('header-check-update').hidden=!hasAvailable;
    $('header-check-update').textContent=navUpdateLabel;
  }
  if($('sidebar-check-update'))$('sidebar-check-update').textContent=navUpdateLabel;
}

if($('auto-update'))$('auto-update').onchange=async()=>{
  const box=$('auto-update');
  if(box.dataset.busy==='1'){box.checked=!box.checked;return;}
  const want=box.checked;
  box.dataset.busy='1';
  setAriaDisabled(box,true);
  try{
    const actual=await invoke('set_auto_update',{enabled:want});
    box.checked=actual;
    if(state.settings)state.settings.auto_update=actual;
    notice(actual?'已开启自动后台更新，发现新版本将在任务空闲时自动升级。':'已关闭自动后台更新，仍可随时手动检查。');
  }catch(error){
    box.checked=!want;
    notice(error,true);
  }finally{
    delete box.dataset.busy;
    setAriaDisabled(box,false);
  }
};

function showUpdateDialog(autoCheck=false){
  const previousFocus=document.activeElement;
  let dialog=document.getElementById('update-dialog');
  if(!dialog){
    dialog=node('dialog',undefined,'profile-dialog update-dialog');
    dialog.id='update-dialog';
    dialog.setAttribute('aria-labelledby','update-dialog-title');
    const header=node('div',undefined,'section-heading');
    const headingWrap=node('div');
    const title=node('h2','软件版本与更新');
    title.id='update-dialog-title';
    title.tabIndex=-1;
    const verLine=node('p',`当前运行版本：v${APP_VERSION}`,'hint');
    verLine.id='dialog-current-version';
    headingWrap.append(title,verLine);
    const closeBtn=button('关闭',()=>dialog.close(),'quiet');
    header.append(headingWrap,closeBtn);

    const body=node('div',undefined,'dialog-body');
    const statusText=node('p','点击“立即检查”从公开的 GitHub Release 检查最新版本，无需登录。','hint');
    statusText.id='dialog-update-status';
    statusText.setAttribute('role','status');
    statusText.setAttribute('aria-live','polite');
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
    dialog.addEventListener('close',()=>{dialog.remove();previousFocus?.focus?.();},{once:true});
    document.body.append(dialog);
  }
  if(latestUpdateStatus)renderUpdateStatus(latestUpdateStatus);
  if(!dialog.open)dialog.showModal();
  document.getElementById('update-dialog-title')?.focus({preventScroll:true});
  if(autoCheck)runUpdateCheck(document.getElementById('dialog-check-btn'));
}

async function runUpdateCheck(triggerBtn){
  const btns=[triggerBtn,$('check-update'),$('header-check-update'),$('sidebar-check-update'),document.getElementById('dialog-check-btn')].filter(Boolean);
  if(btns.some(b=>b.dataset.busy==='1'))return;
  btns.forEach(b=>{b.dataset.busy='1';setAriaDisabled(b,true);});
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
    const found=await invoke('check_update');
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
    const msg=found.downloaded
      ?`新版本 v${found.version} 已在后台下载并验签，可立即安装并重启。`
      :`发现新版本 v${found.version}（当前 v${found.current_version||APP_VERSION}）。`;
    setStatus(msg,found.body||'');
    $('install-update').hidden=false;
    const dialogInstall=document.getElementById('dialog-install-btn');
    if(dialogInstall)dialogInstall.style.display='inline-block';
    notice(found.downloaded?`新版本 v${found.version} 已就绪，点击即可安装重启。`:`发现新版本 v${found.version}，可立即更新。`);
  }catch(err){
    const msg='检查更新失败：'+(err?.message||err);
    setStatus(msg);
    notice(msg,true);
  }finally{
    btns.forEach(b=>{delete b.dataset.busy;setAriaDisabled(b,false);});
  }
}

async function runInstallUpdate(triggerBtn){
  if(!pendingUpdate&&!latestUpdateStatus?.available_version)throw Error('请先检查新版本');
  return action(triggerBtn||$('install-update'),async()=>{
    await invoke('install_update');
    notice('更新包下载校验完成，正在重启应用……');
  });
}

$('check-update').onclick=()=>runUpdateCheck($('check-update'));
$('install-update').onclick=()=>runInstallUpdate($('install-update'));
function openUpdateFromNav(){
  const alreadyFound=Boolean(latestUpdateStatus?.available_version)||['downloading','installing'].includes(latestUpdateStatus?.phase);
  showUpdateDialog(!alreadyFound);
}
if($('header-check-update'))$('header-check-update').onclick=openUpdateFromNav;
if($('sidebar-check-update'))$('sidebar-check-update').onclick=openUpdateFromNav;

(async()=>{
  try{
    if(!window.__TAURI__)throw Error('请通过 Flow2API 桌面应用打开此界面');
    await window.__TAURI__.event.listen('status',event=>updateStatus(event.payload));
    await window.__TAURI__.event.listen('notice',event=>notice(event.payload,String(event.payload).startsWith('无法')));
    await window.__TAURI__.event.listen('account-synced',()=>{refresh().catch(()=>{});});
    await window.__TAURI__.event.listen('check_update',()=>{openUpdateFromNav();});
    await window.__TAURI__.event.listen('update-progress',event=>{
      const received=event.payload.received,total=event.payload.total;
      const msg=total?`已下载 ${(received/1048576).toFixed(1)} / ${(total/1048576).toFixed(1)} MB (${Math.round(received/total*100)}%)`:`已下载 ${(received/1048576).toFixed(1)} MB`;
      $('update-status').textContent=msg;
      const dialogStatus=document.getElementById('dialog-update-status');
      if(dialogStatus)dialogStatus.textContent=msg;
    });
    await window.__TAURI__.event.listen('update-installing',()=>{
      const msg='正在校验签名并安装，当前版本会保留到安装成功。';
      $('update-status').textContent=msg;
      const dialogStatus=document.getElementById('dialog-update-status');
      if(dialogStatus)dialogStatus.textContent=msg;
    });
    await window.__TAURI__.event.listen('update-status',event=>renderUpdateStatus(event.payload));
    renderUpdateStatus(await invoke('update_status'));
    await reloadSettings();
    await restoreDraft();
    state.models=(await invoke('models')).data;
    renderModels();
    $('platform').textContent=navigator.userAgent.includes('Mac')?'macOS · 菜单栏运行':navigator.userAgent.includes('Windows')?'Windows · 托盘运行':'Linux · 托盘运行';
    await refresh();
    loadDevice().catch(error=>notice('设备设置读取失败：'+error,true));
    let polling=false,pollTimer=null;
    function schedulePoll(){
      clearTimeout(pollTimer);
      if(document.hidden)return;
      const active=(state.status?.running||0)+(state.status?.queued||0)>0||state.page==='accounts'||state.page==='tasks';
      pollTimer=setTimeout(async()=>{
        if(document.hidden){schedulePoll();return;}
        if(!polling){
          polling=true;
          try{await refresh();}catch(error){notice(error,true);}finally{polling=false;}
        }
        schedulePoll();
      },active?4000:12000);
    }
    document.addEventListener('visibilitychange',()=>{
      if(!document.hidden){
        if(!polling){
          polling=true;
          refresh().catch(error=>notice(error,true)).finally(()=>{polling=false;schedulePoll();});
        }else{
          schedulePoll();
        }
      }else{
        clearTimeout(pollTimer);
      }
    });
    schedulePoll();
  }catch(error){
    notice(error,true);
  }
})();
