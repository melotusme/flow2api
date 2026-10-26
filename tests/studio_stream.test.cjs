const {test}=require('node:test');
const assert=require('node:assert/strict');
const {consumeStream}=require('../static/assets/studio-core.js');
function response(chunks){return new Response(new ReadableStream({start(controller){for(const chunk of chunks)controller.enqueue(new TextEncoder().encode(chunk));controller.close()}}))}
test('HTTP 200 with stream error must reject',async()=>{
 const r=response(['data: {"error":{"message":"trusted initialization failed"}}\n\ndata: [DONE]\n\n']);
 await assert.rejects(consumeStream(r,()=>{}),/trusted initialization failed/);
});
test('partial stream must not be reported as success',async()=>{
 const r=response(['data: {"choices":[{"delta":{"content":"pending"}}]}\n\n']);
 await assert.rejects(consumeStream(r,()=>{}),/连接提前中断/);
});
test('split SSE boundaries and reasoning are handled separately',async()=>{
 const r=response(['data: {"choices":[{"delta":{"reasoning_content":"准备生成"}}]}\n','\ndata: {"choices":[{"delta":{"content":"![作品](https://example.com/image.jpg)"}}]}\n\ndata: [DO','NE]\n\n']);
 assert.equal(await consumeStream(r,()=>{}),'![作品](https://example.com/image.jpg)');
});
