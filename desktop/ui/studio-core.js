/* Shared, deterministic studio rules. Model capabilities remain the source of truth. */
(function(root){
  'use strict';
  const fields=['type','family','kind','aspect_ratio','spec'];
  const value=(model,field)=>field==='spec'
    ? String(model.capabilities.type==='image'?model.capabilities.resolution:model.capabilities.duration_seconds)
    : String(model.capabilities[field]);
  function choices(models,wanted={}){
    let remaining=models;
    const options={},selected={};
    for(const field of fields){
      options[field]=[...new Set(remaining.map(m=>value(m,field)))];
      selected[field]=options[field].includes(String(wanted[field]))?String(wanted[field]):options[field][0];
      remaining=remaining.filter(m=>value(m,field)===selected[field]);
    }
    const model=remaining.find(m=>m.id===wanted.id)||remaining.find(m=>m.id.includes('-'+(m.capabilities.duration_seconds||'')+'s-'))||remaining[0];
    return {options,selected,model};
  }
  function selection(model){
    return Object.fromEntries([['id',model.id],...fields.map(field=>[field,value(model,field)])]);
  }
  function validate(request,model){
    if(!model)return {field:'model-family',message:'请选择生成模型。'};
    if(!request.prompt.trim())return {field:'prompt',message:'请描述要生成的内容。'};
    if(new TextEncoder().encode(request.prompt).length>100000)return {field:'prompt',message:'描述过长，请缩短到 100 KB 以内。'};
    const c=model.capabilities;
    if(request.images.length<c.min_images||request.images.length>c.max_images)return {field:'images',message:`当前用途需要 ${c.min_images}–${c.max_images} 张参考图，已选择 ${request.images.length} 张。请调整参考图或切换用途。`};
    if(c.requires_video_id&&!request.video_id?.trim())return {field:'video-id',message:'请从本机作品选择视频续写，或填写源视频 media ID。'};
    return null;
  }
  const api={choices,selection,validate};
  if(typeof module==='object'&&module.exports)module.exports=api;
  else root.FlowStudio=api;
})(globalThis);
