(()=>{
 const origin=location.origin;
 for(const [id,value] of [['serverOrigin',origin],['openaiBase',origin+'/v1'],['magpieBase',origin+'/v1']])document.getElementById(id).textContent=value;
 const examples=[
 ['设置环境变量',`export FLOW2API_BASE='${origin}'\nexport FLOW2API_KEY='替换为你的 API Key'`],
 ['获取模型和能力',`curl -fsS "$FLOW2API_BASE/v1/models" \\\n  -H "Authorization: Bearer $FLOW2API_KEY"`],
 ['文字生成图片',`curl -sS --max-time 240 "$FLOW2API_BASE/v1/chat/completions" \\\n  -H "Authorization: Bearer $FLOW2API_KEY" \\\n  -H 'Content-Type: application/json' \\\n  -d '{\n    "model": "gemini-3.1-flash-lite-image-square",\n    "messages": [{"role": "user", "content": "一把黄色陶瓷茶壶，木桌，柔和晨光，无文字"}],\n    "stream": false\n  }'`],
 ['参考图片编辑',`curl -sS --max-time 240 "$FLOW2API_BASE/v1/chat/completions" \\\n  -H "Authorization: Bearer $FLOW2API_KEY" \\\n  -H 'Content-Type: application/json' \\\n  -d '{\n    "model": "gemini-3.1-flash-image-four-three",\n    "messages": [{"role": "user", "content": [\n      {"type": "text", "text": "保持茶壶外观，在旁边增加一只白色茶杯"},\n      {"type": "image_url", "image_url": {"url": "https://example.com/your-reference.jpg"}}\n    ]}],\n    "stream": false\n  }'\n# 图片 URL 替换为实际可访问地址，也支持 data:image/jpeg;base64,...`],
 ['文字生成视频（需账号权限）',`curl -N -sS --max-time 900 "$FLOW2API_BASE/v1/chat/completions" \\\n  -H "Authorization: Bearer $FLOW2API_KEY" \\\n  -H 'Content-Type: application/json' \\\n  -d '{\n    "model": "omni-1.1-flash-4s-landscape",\n    "messages": [{"role": "user", "content": "黄色茶壶冒出轻柔热气，固定镜头，无文字"}],\n    "stream": true\n  }'`],
 ['首尾帧 / 参考图 / 续写',`# 首尾帧：用参考图片编辑示例的 content 结构，按首帧、尾帧顺序传图\n# model: veo-3.1-fast-i2v-4s-landscape\n# 多图参考：model: veo-3.1-fast-r2v-8s-landscape，传 1–3 张图\n# 续写：model: veo-3.1-fast-extend-8s-landscape\n# 在 content 中传入：\n{"type":"image_url","image_url":{"url":"extend://替换为源视频mediaID"}}\n# 请求方式仍为 POST /v1/chat/completions`],
 ];
 const host=document.getElementById('codeExamples');
 examples.forEach(([label,code],i)=>{const toolbar=document.createElement('div');toolbar.className='code-toolbar';const title=document.createElement('strong');title.textContent=label;const button=document.createElement('button');button.className='btn';button.textContent='复制';button.onclick=()=>copyText(code);toolbar.append(title,button);const pre=document.createElement('pre');pre.className='code';pre.textContent=code;pre.id='example'+i;host.append(toolbar,pre)});
 document.getElementById('magpieCurl').textContent=`export MAGPIE_KEY='替换为 Magpie 网关密钥'\ncurl -fsS 'http://127.0.0.1:3425/v1/models' \\\n  -H "Authorization: Bearer $MAGPIE_KEY"\n\n# 已在本机验证的 Flow2API 图片模型\ncurl -sS --max-time 240 'http://127.0.0.1:3425/v1/chat/completions' \\\n  -H "Authorization: Bearer $MAGPIE_KEY" \\\n  -H 'Content-Type: application/json' \\\n  -d '{"model":"flow2api/gemini-3.1-flash-lite-image-square","messages":[{"role":"user","content":"一把蓝色陶瓷茶壶，柔和晨光，无文字"}],"stream":false}' `;
 document.querySelectorAll('[data-copy]').forEach(b=>b.onclick=()=>copyText(document.getElementById(b.dataset.copy).textContent));
})();
