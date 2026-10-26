import unittest
from types import SimpleNamespace
from unittest.mock import AsyncMock
from src.services.token_manager import TokenManager
from src.services.flow_current import CurrentFlowClientMixin

class AccountRuntimeTests(unittest.IsolatedAsyncioTestCase):
    def test_browser_runtime_failure_does_not_invalidate_cookies(self):
        manager = self.manager()
        self.assertTrue(manager._is_transient_network_failure("ego-browser 操作失败: configured space closed"))
        self.assertFalse(manager._is_transient_network_failure("401 UNAUTHENTICATED"))

    def manager(self, valid=True):
        manager = TokenManager.__new__(TokenManager)
        token = SimpleNamespace(id=2, at="flow-frontend-cookie", google_cookies="fixture")
        manager.db = SimpleNamespace(get_token=AsyncMock(return_value=token), update_token=AsyncMock())
        manager.ensure_valid_token = AsyncMock(return_value=token if valid else None)
        manager.flow_client = SimpleNamespace(get_credits=AsyncMock(side_effect=RuntimeError("browser unavailable")))
        return manager

    async def test_failed_auth_is_not_successful_zero_balance(self):
        manager = self.manager(False)
        with self.assertRaisesRegex(RuntimeError, "登录态"):
            await manager.refresh_credits(2)
        manager.db.update_token.assert_not_awaited()

    async def test_upstream_failure_does_not_return_zero(self):
        manager = self.manager()
        with self.assertRaisesRegex(RuntimeError, "browser unavailable"):
            await manager.refresh_credits(2)
        manager.db.update_token.assert_not_awaited()

    async def test_project_creation_keeps_account_binding(self):
        client = CurrentFlowClientMixin()
        client._get_control_plane_timeout = lambda: 30
        client._current_rpc = AsyncMock(return_value="12345678-1234-1234-1234-123456789abc")
        client._extract_flow_project_id = lambda value: value
        await client.create_project("fixture", "test", google_cookies="fixture", token_id=2)
        self.assertEqual(client._current_rpc.call_args.kwargs["token_id"], 2)
