(function(root){
 const safeMediaUrl=value=>{try{const u=new URL(value,location.origin);return ['http:','https:'].includes(u.protocol)?u.href:null}catch{return null}};
 async function consumeStream(response,onChunk){
  const reader=response.body.getReader(),decoder=new TextDecoder();let buffer='',content='',terminal=false;
  function event(block){
   const text=block.split(/\r?\n/).filter(l=>l.startsWith('data:')).map(l=>l.slice(5).trimStart()).join('\n');
   if(!text)return;
   if(text==='[DONE]'){terminal=true;return}
   let data;try{data=JSON.parse(text)}catch{throw new Error('服务返回了无法解析的流式响应')}
   if(data.error)throw new Error(typeof data.error==='string'?data.error:data.error.message||JSON.stringify(data.error));
   for(const c of data.choices||[]){const d=c.delta||c.message||{};if(d.reasoning_content)onChunk(d.reasoning_content);if(d.content){content+=d.content;onChunk(d.content)}}
  }
  try{
   while(true){const {done,value}=await reader.read();if(done){buffer+=decoder.decode();break}buffer+=decoder.decode(value,{stream:true});const blocks=buffer.split(/\r?\n\r?\n/);buffer=blocks.pop();for(const block of blocks)event(block)}
   if(buffer.trim())event(buffer);
   if(!terminal)throw new Error('连接提前中断，尚未收到完成标记。请先查看请求日志，避免重复提交。');
   if(!content.trim())throw new Error('服务没有返回生成结果，请检查请求日志');
   return content;
  }finally{await reader.cancel().catch(()=>{});reader.releaseLock()}
 }
 root.FlowStudioCore={consumeStream,safeMediaUrl};
 if(typeof module!=='undefined')module.exports=root.FlowStudioCore;
})(globalThis);
