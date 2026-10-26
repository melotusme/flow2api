'use strict';
async function loadLibrary(){
 const container=document.getElementById('items');
 try{const r=await fetch('/api/library');if(r.status===401){location.href='/login';return}if(!r.ok)throw Error('读取本机资源失败');const d=await r.json();document.getElementById('directory').textContent=d.directory;document.getElementById('count').textContent=`共 ${d.items.length} 个资源`;container.replaceChildren();
 for(const item of d.items){const card=document.createElement('article');card.className='card';const media=document.createElement(item.type==='video'?'video':'img');media.src=item.url;if(item.type==='video'){media.controls=true;media.preload='metadata'}else{media.alt='本机保存的生成图片';media.loading='lazy'}const info=document.createElement('p');info.className='muted';info.textContent=`${item.type==='video'?'视频':'图片'} · ${(item.size/1024/1024).toFixed(2)} MB · ${new Date(item.created_at).toLocaleString()}`;const name=document.createElement('code');name.textContent=item.filename;const actions=document.createElement('div');actions.className='library-actions';for(const [label,url] of [['打开',item.url],['下载',item.url+'?download=true']]){const a=document.createElement('a');a.className='btn';a.textContent=label;a.href=url;if(label==='打开'){a.target='_blank';a.rel='noopener'}actions.append(a)}const copy=document.createElement('button');copy.className='btn';copy.textContent='复制链接';copy.onclick=()=>copyText(new URL(item.url,location.origin).href);actions.append(copy);card.append(media,info,name,actions);container.append(card)}
 if(!d.items.length)container.textContent='暂无资源，生成成功后会自动出现在这里。';
 }catch(e){document.getElementById('count').textContent=e.message}
}
document.getElementById('refresh').onclick=loadLibrary;loadLibrary();
