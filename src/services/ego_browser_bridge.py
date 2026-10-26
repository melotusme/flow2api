"""Use an explicitly configured, logged-in ego lite Flow tab."""

import asyncio
import json
import re
from urllib.parse import urlparse

from ..core.config import config
from .recaptcha_runtime import TRUSTED_RECAPTCHA_HOOK_SOURCE, build_recaptcha_execute_script

_LOCK = asyncio.Lock()
_MARKER = "FLOW2API_EGO_RESULT:"


async def _run(operation, **arguments):
    settings = config.get_raw_config().get("captcha", {})
    space_id = settings.get("ego_browser_space_id")
    expected_email = str(settings.get("ego_browser_expected_email", "")).strip()
    if not space_id or not expected_email:
        raise RuntimeError("ego_browser 需要配置已授权的 space_id 和 expected_email")
    payload = dict(operation=operation, space_id=int(space_id),
                   page_label=settings.get("ego_browser_page_label", "p1"),
                   expected_email=expected_email, **arguments)
    script = "const input = " + json.dumps(payload) + ";\n" + r"""
// This explicitly configured space is the user's authorized Flow runtime.
// Finishing the setup task returns its tabs to the user and clears Page labels.
const binding = (await listTaskSpaces()).find(space => space.id === input.space_id);
if (!binding) throw new Error('Configured Flow space is closed; reconnect ego lite in settings');
const task = binding.ownership === 'user'
    ? await claimTaskSpace(input.space_id)
    : await taskSpace(input.space_id);
let page;
const tabs = await task.tabs();
const bound = tabs.find(tab => tab.label === input.page_label);
if (bound) {
    page = task.page(bound.label);
} else {
    const flowTabs = tabs.filter(tab => {
        try { return new URL(tab.url).origin === 'https://flow.google.com'; }
        catch { return false; }
    });
    if (flowTabs.length !== 1) throw new Error('Keep exactly one Flow tab in the configured ego space');
    page = flowTabs[0].label ? task.page(flowTabs[0].label) : await task.adopt(flowTabs[0].page);
}
if (new URL(await page.url()).origin !== 'https://flow.google.com') {
    throw new Error('Configured ego tab must already be on flow.google.com');
}
const account = await page.evaluate(() => [...document.querySelectorAll('[aria-label]')]
    .map(el => el.getAttribute('aria-label')).find(text => text.startsWith('Google Account:')) || '');
if (!account.toLowerCase().includes('(' + input.expected_email.toLowerCase() + ')')) {
    throw new Error('ego Flow account does not match configured account; no request sent');
}
let result;
if (input.operation === 'fetch') {
    const response = await page.fetch(input.url, {
        method: input.method, headers: input.headers,
        ...(input.body == null ? {} : {body: input.body}),
        timeout: input.timeout_ms, credentials: 'include'
    });
    result = {status: response.status, text: response.body};
} else if (input.operation === 'captcha') {
    const target = 'https://flow.google.com/project/' + encodeURIComponent(input.project_id);
    const hooked = await page.evaluate(() => !!window.__flow2apiTrustedRecaptchaHookInstalled
        && !!window.__flow2apiRecaptchaActionGuardInstalled);
    if ((await page.url()) !== target || !hooked) {
        // Flags belong to the OLD document; CDP registrations belong to this
        // CLI session. Register before EVERY navigation, even from a hooked page.
        const registration = await page.cdp('Page.addScriptToEvaluateOnNewDocument', {source: input.hook});
        try {
            await page.goto(target, {waitUntil:'domcontentloaded', timeout:30000});
        } finally {
            if (registration?.identifier) await page.cdp('Page.removeScriptToEvaluateOnNewDocument', {
                identifier: registration.identifier
            });
        }
    }
    await page.waitForFunction(() => !!window.grecaptcha?.enterprise, undefined, {timeout:20000});
    const initialized = await page.evaluate(() => !!window.__flow2apiTrustedRecaptchaHookInstalled
        && !!window.__flow2apiRecaptchaActionGuardInstalled);
    if (!initialized) throw new Error('ego captcha initialization missing after navigation; no generation submitted');
    const solved = await page.evaluate(input.execute_script);
    if (!solved || !solved.ok || !solved.token) throw new Error('ego captcha: ' + JSON.stringify(solved));
    result = {token: solved.token, fingerprint: await page.evaluate(() => ({
        user_agent: navigator.userAgent, accept_language: navigator.languages.join(','),
        origin: location.origin, referer: location.href
    }))};
} else throw new Error('Unsupported ego operation');
console.log('FLOW2API_EGO_RESULT:' + JSON.stringify(result));
"""
    async with _LOCK:
        process = await asyncio.create_subprocess_exec(
            "ego-browser", "nodejs", stdin=asyncio.subprocess.PIPE,
            stdout=asyncio.subprocess.PIPE, stderr=asyncio.subprocess.PIPE,
            limit=32 * 1024 * 1024,
        )
        try:
            stdout, stderr = await asyncio.wait_for(process.communicate(script.encode()), timeout=75)
        except (TimeoutError, asyncio.CancelledError):
            process.kill()
            await process.wait()
            raise
    for line in (stdout + b"\n" + stderr).decode().splitlines():
        if line.startswith(_MARKER):
            return json.loads(line[len(_MARKER):])
    # Browser errors can include request data: do not echo the script or raw output.
    first_line = (stderr.decode().splitlines() or [""])[0]
    detail = re.sub(r"[A-Za-z0-9+/_=-]{60,}", "[REDACTED]", first_line)[:240]
    raise RuntimeError(f"ego-browser 操作失败 (exit={process.returncode}): {detail}")


async def fetch_text(method, url, headers=None, body=None, timeout=30):
    if urlparse(url).scheme != "https" or urlparse(url).hostname != "flow.google.com":
        raise ValueError("ego bridge 仅允许 https://flow.google.com")
    allowed_headers = {k: v for k, v in (headers or {}).items()
                       if k.lower() in {"accept", "content-type", "x-same-domain"}}
    if isinstance(body, bytes):
        body = body.decode()
    result = await _run("fetch", method=method, url=url, headers=allowed_headers,
                        body=body, timeout_ms=max(1000, min(int(timeout) * 1000, 60000)))
    if not isinstance(result.get("text"), str):
        raise RuntimeError("ego fetch 返回的响应不是文本")
    return result


async def get_captcha(project_id, action):
    return await _run(
        "captcha", project_id=project_id,
        hook=TRUSTED_RECAPTCHA_HOOK_SOURCE,
        execute_script=build_recaptcha_execute_script(
            "6LdsFiUsAAAAAIjVDZcuLhaHiDn5nnHVXVRQGeMV", action, 20000
        ),
    )
