const {test}=require('node:test');
const assert=require('node:assert/strict');
const {choices,selection,validate}=require('../desktop/ui/studio-core.js');
const catalog=require('../desktop/crates/core/assets/models.json');
const models=Object.entries(catalog).filter(([,m])=>m.config.listed!==false).map(([id,m])=>({id,capabilities:m.capabilities}));

test('all public model variants remain reachable without inventing combinations',()=>{
  for(const model of models){
    const result=choices(models,selection(model));
    assert.equal(result.model.id,model.id);
    assert.ok(models.includes(choices(models,{...selection(model),aspect_ratio:'unsupported'}).model));
  }
});
test('changing image/video workflow preserves compatible dimensions and falls back to real specs',()=>{
  const image=models.find(m=>m.id==='gemini-3.1-flash-lite-image-square');
  const video=choices(models,{...selection(image),type:'video'});
  assert.equal(video.model.capabilities.type,'video');
  const extend=choices(models,{...video.selected,kind:'extend'});
  assert.equal(extend.model.capabilities.requires_video_id,true);
  const back=choices(models,{...extend.selected,type:'image'});
  assert.equal(back.model.capabilities.type,'image');
});
test('submit validation focuses actionable inputs and counts UTF-8 bytes',()=>{
  const image=models.find(m=>m.id==='gemini-3.1-flash-lite-image-square');
  const request={prompt:'茶壶',images:[],video_id:null};
  assert.equal(validate(request,image),null);
  assert.equal(validate({...request,prompt:'  '},image).field,'prompt');
  assert.equal(validate({...request,prompt:'中'.repeat(34000)},image).field,'prompt');
  assert.equal(validate({...request,images:Array(6).fill('reference')},image).field,'images');
  const video=models.find(m=>m.capabilities.requires_video_id);
  assert.equal(validate(request,video).field,'video-id');
  assert.equal(validate({...request,video_id:'media-id'},video),null);
  const frames=models.find(m=>m.capabilities.kind==='i2v');
  assert.equal(validate(request,frames).field,'images');
});
