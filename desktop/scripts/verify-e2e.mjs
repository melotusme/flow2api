#!/usr/bin/env node
/**
 * Flow2API Desktop End-to-End Verification Helper
 *
 * Usage:
 *   node desktop/scripts/verify-e2e.mjs [--base http://127.0.0.1:8000] [--key <API_KEY>] [--stage readiness|image|video]
 *
 * By default runs `--stage readiness` (safe, zero-credit check of /health, /metrics,
 * OpenAI & Gemini model discovery, and error contracts). Pass `--stage image` or
 * `--stage video` after logging in to verify live generation and local media archiving.
 */
import {parseArgs} from 'node:util';

const {values} = parseArgs({
  options: {
    base: {type: 'string', default: process.env.FLOW2API_BASE_URL || 'http://127.0.0.1:8000'},
    key: {type: 'string', default: process.env.FLOW2API_API_KEY || ''},
    stage: {type: 'string', default: 'readiness'},
  },
});

const base = values.base.replace(/\/+$/, '');
const key = values.key.trim();
const stage = values.stage;

async function check(name, fn) {
  try {
    const detail = await fn();
    console.log(`[PASS] ${name}${detail ? ` — ${detail}` : ''}`);
    return detail;
  } catch (error) {
    console.error(`[FAIL] ${name} — ${error.message || error}`);
    process.exitCode = 1;
    return null;
  }
}

async function run() {
  const health = await check('GET /health', async () => {
    const r = await fetch(`${base}/health`);
    if (!r.ok) throw new Error(`HTTP ${r.status}`);
    const body = await r.json();
    if (body.status !== 'healthy') throw new Error(`Unexpected status: ${body.status}`);
    return `service=${body.service}, active_tokens=${body.active_tokens}/${body.total_tokens}`;
  });

  await check('GET /metrics', async () => {
    const r = await fetch(`${base}/metrics`);
    if (!r.ok) throw new Error(`HTTP ${r.status}`);
    const text = await r.text();
    if (!text.includes('flow2api_up 1')) throw new Error('flow2api_up != 1');
    return 'flow2api_up=1';
  });

  if (!key) {
    console.log('[INFO] 未提供 --key 或 FLOW2API_API_KEY，跳过需鉴权的模型发现与生成验证。');
    return;
  }

  await check('GET /v1/models (OpenAI)', async () => {
    const r = await fetch(`${base}/v1/models`, {headers: {Authorization: `Bearer ${key}`}});
    if (!r.ok) throw new Error(`HTTP ${r.status}`);
    const body = await r.json();
    return `${body.data?.length || 0} models`;
  });

  await check('GET /v1/models/aliases', async () => {
    const r = await fetch(`${base}/v1/models/aliases`, {headers: {Authorization: `Bearer ${key}`}});
    if (!r.ok) throw new Error(`HTTP ${r.status}`);
    const body = await r.json();
    return `${body.data?.length || 0} entries`;
  });

  await check('GET /v1beta/models (Gemini)', async () => {
    const r = await fetch(`${base}/v1beta/models`, {headers: {'x-goog-api-key': key}});
    if (!r.ok) throw new Error(`HTTP ${r.status}`);
    const body = await r.json();
    return `${body.models?.length || 0} models`;
  });

  if (stage === 'image') {
    await check('POST /v1/chat/completions (Image Generation)', async () => {
      const r = await fetch(`${base}/v1/chat/completions`, {
        method: 'POST',
        headers: {
          Authorization: `Bearer ${key}`,
          'Content-Type': 'application/json',
        },
        body: JSON.stringify({
          model: 'gemini-3.1-flash-lite-image-square',
          stream: false,
          messages: [{role: 'user', content: '一把黄色陶瓷茶壶摆在木桌上，自然晨光，无文字'}],
        }),
      });
      if (!r.ok) throw new Error(`HTTP ${r.status}: ${(await r.text()).slice(0, 300)}`);
      const body = await r.json();
      const content = body.choices?.[0]?.message?.content || '';
      if (!content.includes('/media/')) throw new Error(`Missing media URL in response: ${content}`);
      return content.trim();
    });
  } else if (stage === 'video') {
    await check('POST /v1/chat/completions (Video Generation)', async () => {
      const r = await fetch(`${base}/v1/chat/completions`, {
        method: 'POST',
        headers: {
          Authorization: `Bearer ${key}`,
          'Content-Type': 'application/json',
        },
        body: JSON.stringify({
          model: 'veo-3.1-fast-t2v-landscape',
          stream: false,
          messages: [{role: 'user', content: '清晨海面微风吹拂，阳光闪烁，平稳镜头'}],
        }),
      });
      if (!r.ok) throw new Error(`HTTP ${r.status}: ${(await r.text()).slice(0, 300)}`);
      const body = await r.json();
      const content = body.choices?.[0]?.message?.content || '';
      if (!content.includes('/media/')) throw new Error(`Missing media URL in response: ${content}`);
      return content.trim();
    });
  }
}

run();
