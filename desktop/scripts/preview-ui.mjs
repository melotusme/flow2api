#!/usr/bin/env node
// Isolated UI fixture. No Tauri privileges, Google requests or real user data.
import http from 'node:http';
import {readFile} from 'node:fs/promises';
const ui=new URL('../ui/',import.meta.url);
const catalog=JSON.parse(await readFile(new URL('../crates/core/assets/models.json',import.meta.url),'utf8'));
const models={data:Object.entries(catalog).filter(([,m])=>m.config.listed!==false).map(([id,m])=>({id,capabilities:m.capabilities}))};
const fixture=`
'use strict';
const sample='data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+aXRkAAAAASUVORK5CYII=';
const accounts=[
  {id:'demo',email:'creator@example.test',enabled:true,priority:100,note:'AI Ultra 主力',image_enabled:true,video_enabled:true,music_enabled:true,music_credits:30620,music_plan:'member',music_last_synced:new Date().toISOString(),disabled_until:null,disabled_reason:null,last_error:null,consecutive_failures:0,project_id:'proj-demo',last_synced:new Date().toISOString(),credits:25050,browser_bridge:true},
  {id:'backup',email:'backup@example.test',enabled:true,priority:0,note:'',image_enabled:true,video_enabled:false,music_enabled:true,music_credits:null,music_plan:'',music_last_synced:null,disabled_until:null,disabled_reason:null,last_error:'PUBLIC_ERROR_RATE_LIMIT_EXCEEDED',consecutive_failures:2,project_id:'',last_synced:new Date(Date.now()-3600000).toISOString(),credits:1200,browser_bridge:true},
];
const account=accounts[0];
const jobs=Array.from({length:12},(_,i)=>({id:'job-'+i,state:i===0?'queued':i===1?'failed':'succeeded',stage:i===0?'等待执行':i===1?'连接中断，请打开 Flow 后再次编辑':'已保存',error:i===1?'浏览器连接已断开，请打开 Flow 标签页':null,created_at:new Date(Date.now()-i*60000).toISOString(),updated_at:new Date(Date.now()-i*60000).toISOString(),request:{model:i===3?'flow-music-standard':'gemini-3.1-flash-lite-image-square',prompt:i===3?'Cheerful Mandarin children song, ukulele, glockenspiel, 110 BPM':'测试草稿 '+i+'：一只黄色陶瓷茶壶，摆在木桌上，窗边自然光。',images:i===3?[]:[sample],account_id:'demo',video_id:null,lyrics:i===3?'[Verse]\\n小太阳挂在天边\\n[Chorus]\\n今天也要开心一点':null,duration_seconds:i===3?60:null,instrumental:false},results:[]}));
let online=true,musicOnline=true,service=true,submissions=0,slowSubmit=0,denyDevice=false;
let settings={port:8173,api_key:'fixture-key-not-a-credential',max_concurrent:1,image_timeout_seconds:300,video_timeout_seconds:1500,music_timeout_seconds:600,browser_executable:'',proxy_url:'',auto_update:true};
const media={filename:'fixture.svg',kind:'image',size:2048,created_at:new Date().toISOString(),prompt:'测试作品：山间日落',model:'gemini-3.1-flash-lite-image-square',task_id:'job-2',media_id:'fixture-media',title:'',lyrics:'',sound_prompt:''};
const audioMedia={filename:'fixture.wav',kind:'audio',size:44144,created_at:new Date(Date.now()-90000).toISOString(),prompt:'Cheerful Mandarin children song, ukulele, glockenspiel, 110 BPM',model:'flow-music-standard',task_id:'job-3',media_id:'clip-demo-001',title:'晴天小步舞',lyrics:'[Verse]\\n小太阳挂在天边\\n微风轻轻吹过窗前\\n\\n[Chorus]\\n今天也要开心一点\\n把好心情唱给明天',sound_prompt:'Cheerful Mandarin children song, ukulele, 110 BPM'};
jobs[2].results=[media];
jobs[3].results=[audioMedia];
const listeners={};
const status=()=>({service:service?'running':'stopped',address:service?location.origin:null,running:jobs.filter(t=>t.state==='running').length,queued:jobs.filter(t=>t.state==='queued').length,succeeded:jobs.filter(t=>t.state==='succeeded').length,failed:1,total_accounts:accounts.length,accounts:accounts.filter(a=>a.enabled).length,ready_accounts:online?1:0,music_ready_accounts:musicOnline?1:0,browser:online||musicOnline?'已连接 · '+(online?'Flow 1':'')+(online&&musicOnline?' · ':'')+(musicOnline?'Music 1':''):'浏览器未连接',data_dir:'临时交互夹具（未使用真实数据）'});
window.preview={jobs,account,accounts,get submissions(){return submissions},setOnline(value){online=value},setMusicOnline(value){musicOnline=value},setSlowSubmit(value){slowSubmit=value},setDenyDevice(value){denyDevice=value},complete(){const task=jobs.find(t=>t.state==='queued');if(task){task.state='succeeded';task.stage='已保存';task.results=[media]}},emit(name,payload){listeners[name]?.({payload})}};
window.__TAURI__={event:{listen:async(name,callback)=>{listeners[name]=callback}},core:{invoke:async(name,args={})=>{
  switch(name){
    case 'models':return ${JSON.stringify(models)};
    case 'settings':return settings;
    case 'magpie_status':return {configured:false,models:30,base_url:'http://127.0.0.1:'+settings.port,config_path:'fixture/providers.json'};
    case 'connect_magpie':if(denyDevice)throw Error('夹具：未找到 Magpie 配置，请先安装并打开 Magpie');return {configured:true,models:30,base_url:'http://127.0.0.1:'+settings.port,backup_path:'fixture/backups/providers-before-flow2api.json'};
    case 'accounts':return accounts.map(a=>({...a}));
    case 'tasks':return jobs.map(t=>({...t,request:{...t.request,images:[]}}));
    case 'task_detail':return structuredClone(jobs.find(t=>t.id===args.id));
    case 'status':return status();
    case 'browser_connections':return [{account_id:'demo',connected:online,state:online?'ready':'disconnected',message:online?'身份与余额校验通过':'连接已断开'}];
    case 'music_connections':return [{account_id:'demo',connected:musicOnline,state:musicOnline?'ready':'disconnected',message:musicOnline?'Flow Music 身份与积分校验通过':'连接已断开'}];
    case 'library':return [media,audioMedia];
    case 'autostart_enabled':return false;
    case 'webdav_config':if(denyDevice)throw Error('夹具：系统凭据库不可用');return {url:'',username:'',has_password:false};
    case 'media_reference':return sample;
    case 'cancel_task':{const task=jobs.find(t=>t.id===args.id);if(task.state!=='queued')throw Error('只能取消排队任务');task.state='cancelled';task.stage='已取消，未提交到 Flow';return task;}
    case 'submit':submissions++;await new Promise(r=>setTimeout(r,slowSubmit));const task={id:'new-'+submissions,state:'queued',stage:'等待执行',created_at:new Date().toISOString(),updated_at:new Date().toISOString(),request:structuredClone(args.request),results:[]};jobs.unshift(task);return task;
    case 'start_service':service=true;return status();
    case 'stop_service':service=false;return status();
    case 'save_settings':settings=args.settings;return;
    case 'save_account':{const target=accounts.find(a=>a.id===args.id)||account;target.enabled=args.enabled;if(args.priority!==undefined)target.priority=args.priority;if(args.note!==undefined)target.note=args.note.trim();if(args.imageEnabled!==undefined)target.image_enabled=args.imageEnabled;if(args.videoEnabled!==undefined)target.video_enabled=args.videoEnabled;if(args.musicEnabled!==undefined)target.music_enabled=args.musicEnabled;if(args.clearError){target.last_error=null;target.consecutive_failures=0;}if(args.resetProject)target.project_id='';return {...target};}
    case 'temp_disable_account':{const target=accounts.find(a=>a.id===args.id)||account;target.disabled_until=args.minutes>0?new Date(Date.now()+args.minutes*60000).toISOString():null;target.disabled_reason=args.minutes>0?(args.reason||\`手动临时禁用 \${args.minutes} 分钟\`):null;return {...target};}
    case 'delete_account':{const idx=accounts.findIndex(a=>a.id===args.id);if(idx>=0)accounts.splice(idx,1);return;}
    case 'sync_account':{const target=accounts.find(a=>a.id===args.id)||account;target.last_synced=new Date().toISOString();return {...target};}
    case 'sync_music_account':{const target=accounts.find(a=>a.id===args.id)||account;target.music_last_synced=new Date().toISOString();return {...target};}
    case 'update_status':return {phase:'idle',message:''};
    case 'check_update':case 'check_private_update':return null;
    case 'list_chrome_profiles':return [
      {data_dir:'fixture/User Data',profile:'Profile 6',name:'Ultra Studio',email:'creator@example.test',gaia_name:'Creator',is_last_used:true,developer_mode:true,extension_installed:true,extension_version:'0.4.1',tampermonkey_installed:true,tampermonkey_allow_user_scripts:false},
      {data_dir:'fixture/User Data',profile:'Default',name:'Personal',email:'backup@example.test',gaia_name:'Backup',is_last_used:false,developer_mode:false,extension_installed:false,extension_version:null,tampermonkey_installed:true,tampermonkey_allow_user_scripts:false},
    ];
    case 'install_chrome_extension_for_profile':return {dir:'fixture/chrome-extension',profile:args.profile||null,opened_browser:true,opened_folder:true};
    case 'open_url_in_chrome_profile':return args.profile||'Default';
    case 'open_chrome_extension_dir':return 'fixture/chrome-extension';
    case 'open_flow_page':case 'open_flow_music_page':case 'open_userscript_url':case 'open_data':case 'open_media':return;
    default:throw Error('夹具未实现命令：'+name);
  }
}}};
`;
const server=http.createServer(async(req,res)=>{
  try{
    const path=new URL(req.url,'http://localhost').pathname;
    if(path==='/fixture.js'){res.setHeader('Content-Type','text/javascript');res.end(fixture);return;}
    if(path==='/media/fixture.svg'){res.setHeader('Content-Type','image/svg+xml');res.end('<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 640 420"><rect width="640" height="420" fill="#233d43"/><circle cx="450" cy="130" r="58" fill="#e2b880"/><path d="M0 420V310L220 120L440 420M180 420L410 210L640 360V420" fill="#83bba9"/><text x="24" y="390" fill="#fff" font-size="20">UI fixture · no Google generation</text></svg>');return;}
    if(path==='/media/fixture.wav'){const n=8000,buf=Buffer.alloc(44+n);buf.write('RIFF',0);buf.writeUInt32LE(36+n,4);buf.write('WAVEfmt ',8);buf.writeUInt32LE(16,16);buf.writeUInt16LE(1,20);buf.writeUInt16LE(1,22);buf.writeUInt32LE(8000,24);buf.writeUInt32LE(8000,28);buf.writeUInt16LE(1,32);buf.writeUInt16LE(8,34);buf.write('data',36);buf.writeUInt32LE(n,40);buf.fill(128,44);res.setHeader('Content-Type','audio/wav');res.end(buf);return;}
    const files={'/':'index.html','/index.html':'index.html','/app.js':'app.js','/studio-core.js':'studio-core.js','/app.css':'app.css'};
    if(!files[path]){res.writeHead(404);res.end();return;}
    let data=await readFile(new URL(files[path],ui),'utf8');
    if(path==='/'||path==='/index.html')data=data.replace('<script src="studio-core.js">','<script src="fixture.js"></script><script src="studio-core.js">');
    res.setHeader('Content-Type',path.endsWith('.js')?'text/javascript':path.endsWith('.css')?'text/css':'text/html; charset=utf-8');res.end(data);
  }catch(error){res.writeHead(500);res.end(error.message);}
});
server.listen(Number(process.env.FLOW2API_PREVIEW_PORT||8173),'127.0.0.1',()=>console.log('Isolated UI fixture: http://127.0.0.1:'+server.address().port));
