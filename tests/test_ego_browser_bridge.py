import unittest
from types import SimpleNamespace
from unittest.mock import AsyncMock, patch

from src.core.config import config
from src.services import ego_browser_bridge as bridge
from src.services.flow_current import CurrentFlowClientMixin


class EgoBrowserBridgeTests(unittest.IsolatedAsyncioTestCase):
    async def test_fetch_rejects_other_origins_before_using_browser(self):
        with patch.object(bridge, "_run", AsyncMock()) as run:
            with self.assertRaises(ValueError):
                await bridge.fetch_text("GET", "https://other.example/")
            run.assert_not_awaited()

    async def test_browser_owns_cookie_and_fingerprint_headers(self):
        with patch.object(bridge, "_run", AsyncMock(return_value={"status": 200, "text": "ok"})) as run:
            await bridge.fetch_text("POST", "https://flow.google.com/rpc", {
                "Cookie": "must-not-copy", "User-Agent": "must-not-spoof",
                "Content-Type": "application/json",
            }, "{}")
            self.assertEqual(run.call_args.kwargs["headers"], {"Content-Type": "application/json"})

    async def test_cli_console_results_on_stderr_are_parsed(self):
        process = SimpleNamespace(communicate=AsyncMock(return_value=(b"", b'FLOW2API_EGO_RESULT:{"status":200,"text":"ok"}\n')), returncode=0)
        with patch.dict(config._config.setdefault("captcha", {}), {
            "ego_browser_space_id": 10, "ego_browser_expected_email": "test@example.com",
        }), patch.object(bridge.asyncio, "create_subprocess_exec", AsyncMock(return_value=process)):
            self.assertEqual(await bridge._run("fetch"), {"status": 200, "text": "ok"})

    async def test_other_token_cannot_use_bound_browser_account(self):
        client = CurrentFlowClientMixin()
        client._get_runtime_config = lambda: config
        client.db = SimpleNamespace(get_token=AsyncMock(return_value=SimpleNamespace(login_account="other@example.com", email="other@example.com")))
        with patch.dict(config._config.setdefault("captcha", {}), {
            "captcha_method": "ego_browser", "ego_browser_expected_email": "bound@example.com",
        }):
            with self.assertRaisesRegex(RuntimeError, "同一账号"):
                await client._current_rpc(rpc_id="test", argument=[], token_id=2)

class EgoNavigationRegressionTests(unittest.IsolatedAsyncioTestCase):
    async def test_hook_survives_project_change_in_new_cli_session(self):
        """Run the real CLI script with document-scoped flags/session-scoped CDP."""
        import subprocess
        harness = r'''
const vm = require('node:vm');
let url = 'https://flow.google.com/project/old';
let initScript = null;
let context;
function newDocument(hooked) {
    context = vm.createContext({window: {}, navigator: {userAgent:'test',languages:['en']},
        location: {origin:'https://flow.google.com',href:url}, document: {
        querySelectorAll: () => [{getAttribute: () => 'Google Account: Test (test@example.com)'}]
    }});
    if(hooked) vm.runInContext(inputHook, context);
}
const inputHook = "window.__flow2apiTrustedRecaptchaHookInstalled=true;window.__flow2apiRecaptchaActionGuardInstalled=true";
newDocument(true);
const page = {
    url: async () => url,
    evaluate: async (code) => vm.runInContext(typeof code === 'function' ? '('+code.toString()+')()' : code, context),
    cdp: async (method, args) => {if(method==='Page.addScriptToEvaluateOnNewDocument') initScript=args.source; return {};},
    goto: async (target) => {url=target;newDocument(false);if(initScript) vm.runInContext(initScript,context);},
    waitForFunction: async () => {},
};
const taskSpace = async () => ({page: () => page});
const returned = RETURNED_SPACE;
let claimed=false, adopted=false;
const listTaskSpaces = async () => [{id:10,ownership:returned?'user':'agent'}];
const task = {page: () => page,tabs:async()=>[returned?{url,page}:{label:'p1',url,page}],adopt:async()=>{adopted=true;return page;}};
const claimTaskSpace = async()=>{claimed=true;return task;};
const script = require('node:fs').readFileSync(0,'utf8');
new (Object.getPrototypeOf(async function(){}).constructor)('taskSpace', 'listTaskSpaces', 'claimTaskSpace', script)(async()=>task,listTaskSpaces,claimTaskSpace)
.then(()=>{if(returned && (!claimed || !adopted)) throw new Error('Returned space was not restored');})
.catch(e => {console.error(e.message);process.exitCode=1});
'''
        async def communicate(script):
            for returned in ('false', 'true'):
                result = subprocess.run(['node', '-e', harness.replace('RETURNED_SPACE', returned)], input=script, capture_output=True)
                self.assertEqual(result.returncode, 0, result.stderr.decode())
            process.returncode = result.returncode
            return result.stdout, result.stderr
        process = SimpleNamespace(communicate=communicate, returncode=None)
        with patch.dict(config._config.setdefault('captcha', {}), {
            'ego_browser_space_id': 10, 'ego_browser_expected_email': 'test@example.com',
        }), patch.object(bridge.asyncio, 'create_subprocess_exec', AsyncMock(return_value=process)):
            result = await bridge._run('captcha', project_id='new',
                hook="window.__flow2apiTrustedRecaptchaHookInstalled=true;window.__flow2apiRecaptchaActionGuardInstalled=true",
                execute_script="window.__flow2apiRecaptchaActionGuardInstalled ? ({ok:true,token:'test-token',fingerprint:{}}) : ({ok:false,error:'trusted reCAPTCHA action guard is unavailable'})")
            self.assertEqual(result['token'], 'test-token')
