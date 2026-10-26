'use strict';
const $=id=>document.getElementById(id);
const state={models:[],selected:null,images:[],busy:false,ownedKey:null,connection:null};
const KIND={image:'图片生成 / 编辑',t2v:'文生视频',i2v:'首尾帧视频',r2v:'参考图视频',omni:'Omni 视频',extend:'视频续写'};
const text=(tag,value,cls)=>{const el=document.createElement(tag);el.textContent=value;if(cls)el.className=cls;return el};
const price=m=>m.capabilities.pricing.credits==null?'费用待核实':`${m.capabilities.pricing.credits} credits / ${m.capabilities.pricing.unit}`;
const variant=m=>[m.capabilities.aspect_ratio,m.capabilities.duration_seconds?`${m.capabilities.duration_seconds} 秒`:m.capabilities.resolution].join(' · ');
function error(message){$('validationError').hidden=!message;$('validationError').textContent=message||''}
function tooltipText(m){const c=m.capabilities;return `${c.family} · ${variant(m)}\n${c.summary}\n输入图片：${c.min_images}–${c.max_images} 张${c.requires_video_id?'；需要源视频 ID':''}\n费用：${price(m)}\n${c.pricing.note}\n${c.access_note}`}
function showTip(m,item){const tip=$('modelTooltip');tip.textContent=tooltipText(m);tip.hidden=false;item.setAttribute('aria-describedby','modelTooltip');const r=item.getBoundingClientRect();const w=Math.min(340,innerWidth-24);tip.style.maxWidth=w+'px';tip.style.left=Math.max(12,Math.min(r.right+10,innerWidth-w-12))+'px';tip.style.top=Math.max(12,Math.min(r.top,innerHeight-tip.offsetHeight-12))+'px'}
function hideTip(){ $('modelTooltip').hidden=true;document.querySelectorAll('[aria-describedby="modelTooltip"]').forEach(el=>el.removeAttribute('aria-describedby')) }
function renderCatalog(){
 const q=$('modelSearch').value.trim().toLowerCase(),filter=$('modelFilter').value;const list=$('modelSidebar');list.replaceChildren();
 const models=state.models.filter(m=>(filter==='all'||m.capabilities.kind===filter)&&`${m.id} ${m.description} ${variant(m)} ${KIND[m.capabilities.kind]}`.toLowerCase().includes(q));
 $('modelCount').textContent=`${models.length} / ${state.models.length}`;let group='';
 for(const m of models){const c=m.capabilities;if(c.family!==group){group=c.family;list.append(text('div',group,'catalog-group'))}const button=text('button','','model-item');button.type='button';button.dataset.model=m.id;button.classList.toggle('active',state.selected?.id===m.id);button.setAttribute('aria-pressed',String(state.selected?.id===m.id));button.setAttribute('aria-label',`${c.family} ${variant(m)} ${KIND[c.kind]}`);
 const row=text('div','','model-row');row.append(text('strong',variant(m)),text('span',KIND[c.kind],'pill'));button.append(row,text('small',price(m)));
 button.addEventListener('click',()=>selectModel(m));button.addEventListener('mouseenter',()=>showTip(m,button));button.addEventListener('mouseleave',hideTip);button.addEventListener('focus',()=>showTip(m,button));button.addEventListener('blur',hideTip);button.addEventListener('keydown',e=>{if(e.key==='Escape')hideTip()});list.append(button)}
 if(!models.length)list.append(text('p','没有匹配模型，试试其他关键词。','muted'));
}
function selectModel(m){
 if(state.busy){notify('当前正在生成，请等待任务结束后切换模型');return}
 const c=m.capabilities;state.selected=m;state.images=state.images.slice(0,c.max_images);error('');hideTip();renderCatalog();
 $('selectedModelName').textContent=c.family;$('selectedModelId').textContent=m.id;$('selectedModelInfo').textContent=c.summary;$('selectedKind').textContent=KIND[c.kind];
 const detail=$('modelDetail');detail.replaceChildren();for(const [label,value] of [['画面规格',variant(m)],['参考图片',c.supports_images?`${c.min_images}–${c.max_images} 张`:'不使用'],['参考费用',price(m)]]){const el=text('div','','detail-cell');el.append(text('small',label),text('strong',value));detail.append(el)}
 $('modelNotice').textContent=`${c.access_note} ${c.pricing.note}${c.pricing.credits==null?' 此规格尚无已核实价格。':''}${c.kind==='image'?' 参考图数量是工作台限制。':''}`;
 $('imageUploadGroup').hidden=!c.supports_images;$('videoIdGroup').hidden=!c.requires_video_id;$('imageLimit').textContent=c.min_images?`必需 ${c.min_images}–${c.max_images} 张`:`可选，最多 ${c.max_images} 张`;
 $('costHint').textContent=`${price(m)} · 每次生成 1 个输出${c.supports_images?'；带参考图时实际扣费以 Flow 为准':''}`;
 $('btnGenerate').disabled=false;$('btnGenerate').textContent=c.type==='image'?'生成图片':'生成视频';renderImages();
}
async function loadModels(){
 if(state.busy)return;error('');let base;
 try{base=new URL($('baseUrl').value);if(!['http:','https:'].includes(base.protocol))throw Error('仅支持 HTTP(S) 服务地址')}catch{error('请输入有效的服务地址');return}
 base=base.href.replace(/\/$/,'').replace(/\/v1$/,'');$('baseUrl').value=base;
 try{
  let key;
  if(base===location.origin){const config=await fetch('/api/admin/config',{credentials:'same-origin'});if(config.status===401){location.href='/login';return}if(!config.ok)throw Error('无法读取本机连接配置');const d=await config.json();state.ownedKey=d.api_key;key=state.ownedKey;$('apiKey').value='';$('apiKey').disabled=true;$('apiKey').placeholder='已使用本机管理员 API Key'}else{state.ownedKey=null;$('apiKey').disabled=false;key=$('apiKey').value.trim();if(!key)throw Error('请填写目标服务的 API Key，再重新连接')}
  const r=await fetch(`${base}/v1/models`,{headers:{Authorization:`Bearer ${key}`},credentials:'omit'});if(!r.ok)throw Error(`HTTP ${r.status}：请检查地址和 API Key`);const d=await r.json();
  state.models=(d.data||[]).filter(m=>m.capabilities).sort((a,b)=>a.capabilities.family.localeCompare(b.capabilities.family)||a.id.localeCompare(b.id));if(!state.models.length)throw Error('当前服务没有提供模型能力信息');
  state.connection={base,key};
  $('connectionState').textContent=`· 已连接 · ${state.models.length} 个模型`;
  const previous=state.models.find(m=>m.id===state.selected?.id);selectModel(previous||state.models.find(m=>m.id==='gemini-3.1-flash-lite-image-square')||state.models[0]);
 }catch(e){error(e.message);$('connectionState').textContent='· 连接失败';$('btnGenerate').disabled=true;state.connection=null;state.models=[];state.selected=null;renderCatalog()}
}
async function handleFiles(files){
 if(state.busy||!state.selected)return;const c=state.selected.capabilities;
 for(const file of files){if(state.images.length>=c.max_images){notify(`此模型最多支持 ${c.max_images} 张图片`);break}if(!file.type.startsWith('image/')){notify('请选择图片文件');continue}if(file.size>15*1024*1024){error('单张参考图不能超过 15 MB');continue}
 const modelId=state.selected.id;const value=await new Promise((resolve,reject)=>{const r=new FileReader();r.onload=()=>resolve(r.result);r.onerror=()=>reject(Error('图片读取失败'));r.readAsDataURL(file)});if(state.busy||state.selected?.id!==modelId)return;state.images.push(value);renderImages()}
}
function renderImages(){const list=$('imagePreviewList');list.replaceChildren();state.images.forEach((src,i)=>{const box=text('div','','upload-preview'),img=document.createElement('img'),button=text('button','×');img.src=src;img.alt=`参考图 ${i+1}`;button.type='button';button.setAttribute('aria-label',`移除参考图 ${i+1}`);button.disabled=state.busy;button.onclick=()=>{state.images.splice(i,1);renderImages()};box.append(img,button);list.append(box)})}
function makeBody(){const prompt=$('promptInput').value.trim(),m=state.selected;if(!state.connection)throw Error('请先连接服务');if(!m)throw Error('请选择模型');if(!prompt)throw Error('请先描述想要生成的画面');const c=m.capabilities;if(state.images.length<c.min_images)throw Error(`此模型至少需要 ${c.min_images} 张参考图`);if(state.images.length>c.max_images)throw Error(`此模型最多支持 ${c.max_images} 张参考图`);if(c.requires_video_id&&!$('videoMediaId').value.trim())throw Error('请填写源视频 media ID');const content=[{type:'text',text:prompt}];state.images.forEach(url=>content.push({type:'image_url',image_url:{url}}));if(c.requires_video_id)content.push({type:'image_url',image_url:{url:'extend://'+$('videoMediaId').value.trim()}});return {model:m.id,messages:[{role:'user',content}],stream:true}}
function renderResult(content){const el=$('outputResult');el.replaceChildren();let count=0;const refs=[...content.matchAll(/!\[[^\]]*\]\(([^)]+)\)/g)].map(m=>({url:m[1],type:'image'}));for(const m of content.matchAll(/<video[^>]+src=['"]([^'"]+)['"][^>]*>/gi))refs.push({url:m[1],type:'video',id:m[0].match(/data-media-id=['"]([^'"]+)/)?.[1]});
 for(const ref of refs){const url=FlowStudioCore.safeMediaUrl(ref.url);if(!url)continue;const figure=document.createElement('figure'),media=document.createElement(ref.type==='image'?'img':'video');media.src=url;if(ref.type==='image'){media.alt='生成的作品';media.loading='lazy'}else{media.controls=true;media.preload='metadata'}const caption=document.createElement('figcaption'),link=text('a','打开原图 / 下载 ↗','btn');link.href=url;link.target='_blank';link.rel='noopener noreferrer';caption.append(text('small',`作品 ${++count}${new URL(url,location.href).pathname.startsWith('/media/')?' · 已保存到本机':''}`),link);figure.append(media,caption);if(ref.id){const b=text('button','复制视频 media ID','btn');b.onclick=()=>copyText(ref.id);figure.append(b)}el.append(figure)}
 if(!count)throw Error(content.match(/(?:生成失败|错误|Error)[\s\S]*/i)?.[0]||'未找到图片或视频结果，请检查过程日志');return count;
}
async function generate(){
 if(state.busy)return;let body;try{body=makeBody()}catch(e){error(e.message);return}error('');state.busy=true;hideTip();$('btnGenerate').disabled=true;$('btnGenerate').textContent='正在生成…';$('imageFileInput').disabled=true;renderImages();$('statusText').textContent='生成进行中';$('outputResult').replaceChildren(text('div','正在生成，请保留页面。视频通常需要更长时间。','output-empty'));$('outputLog').textContent=`模型：${body.model}\n`;const started=Date.now();const timer=setInterval(()=>$('elapsed').textContent=`${Math.floor((Date.now()-started)/1000)} 秒`,1000);
 try{const r=await fetch(`${state.connection.base}/v1/chat/completions`,{method:'POST',headers:{Authorization:`Bearer ${state.connection.key}`,'Content-Type':'application/json'},body:JSON.stringify(body),credentials:'omit'});if(!r.ok){const d=await r.text();throw Error(`HTTP ${r.status}: ${d.slice(0,600)}`)}const content=await FlowStudioCore.consumeStream(r,t=>{$('outputLog').textContent+=t});renderResult(content);$('statusText').textContent='作品已完成';$('elapsed').textContent=`耗时 ${((Date.now()-started)/1000).toFixed(1)} 秒`;
 }catch(e){error(e.message);$('statusText').textContent='生成未完成';$('logDetails').open=true;$('outputLog').textContent+=`\n失败：${e.message}`;$('outputResult').replaceChildren(text('div','本次未返回成品。修正上方提示后再试。','output-empty'));
 }finally{clearInterval(timer);state.busy=false;$('imageFileInput').disabled=false;$('btnGenerate').disabled=false;$('btnGenerate').textContent=state.selected.capabilities.type==='image'?'生成图片':'生成视频';renderImages()}
}
$('modelSearch').addEventListener('input',()=>{hideTip();renderCatalog()});$('modelFilter').addEventListener('change',()=>{hideTip();renderCatalog()});$('connectBtn').onclick=loadModels;$('btnGenerate').onclick=generate;$('imageFileInput').onchange=()=>{handleFiles($('imageFileInput').files).catch(e=>error(e.message));$('imageFileInput').value=''};
$('imageUploadArea').ondragover=e=>e.preventDefault();$('imageUploadArea').ondrop=e=>{e.preventDefault();handleFiles(e.dataTransfer.files).catch(e=>error(e.message))};
window.addEventListener('scroll',hideTip,true);window.addEventListener('resize',hideTip);window.addEventListener('beforeunload',e=>{if(state.busy){e.preventDefault();e.returnValue=''}});$('baseUrl').addEventListener('input',()=>{state.connection=null;$('btnGenerate').disabled=true;$('apiKey').disabled=false;$('apiKey').placeholder='填写目标服务的 API Key';$('connectionState').textContent='· 设置已改，请重新连接'});$('apiKey').addEventListener('input',()=>{if(!$('apiKey').disabled){state.connection=null;$('btnGenerate').disabled=true;$('connectionState').textContent='· 设置已改，请重新连接'}});$('baseUrl').value=location.origin;loadModels();
