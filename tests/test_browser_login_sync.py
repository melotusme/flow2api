import unittest
from types import SimpleNamespace
from unittest.mock import AsyncMock, patch

from fastapi import FastAPI
from fastapi.testclient import TestClient
from src.api import admin
from src.services import protocol_login


class BrowserLoginSyncTests(unittest.TestCase):
    def setUp(self):
        self.app = FastAPI()
        self.app.include_router(admin.router)
        self.client = TestClient(self.app)
        self.body = {"login_account": "test@example.com", "google_cookies": "fixture"}

    def test_requires_admin_auth(self):
        self.assertEqual(self.client.post("/api/browser-login/sync", json=self.body).status_code, 401)

    def test_cookie_owner_validation_before_write(self):
        self.app.dependency_overrides[admin.verify_admin_token] = lambda: "test-session"
        database = SimpleNamespace(get_token_by_email=AsyncMock(), add_token=AsyncMock())
        with patch.object(admin, "db", database), patch.object(admin, "proxy_manager", None), patch.object(protocol_login.protocol_loginer, "login", AsyncMock(return_value={"success": False, "error": "account mismatch"})):
            response = self.client.post("/api/browser-login/sync", json=self.body)
        self.assertEqual(response.status_code, 400)
        database.add_token.assert_not_awaited()

    def test_existing_account_preserves_settings(self):
        self.app.dependency_overrides[admin.verify_admin_token] = lambda: "test-session"
        database = SimpleNamespace(get_token_by_email=AsyncMock(return_value=SimpleNamespace(id=7)))
        manager = SimpleNamespace(update_token=AsyncMock())
        validated = {"success": True, "email": "test@example.com", "google_cookies": "validated", "session_token": "fixture"}
        with patch.object(admin, "db", database), patch.object(admin, "token_manager", manager), patch.object(admin, "proxy_manager", None), patch.object(protocol_login.protocol_loginer, "login", AsyncMock(return_value=validated)):
            response = self.client.post("/api/browser-login/sync", json=self.body)
        self.assertEqual(response.json()["updated"], 1)
        self.assertNotIn("image_concurrency", manager.update_token.call_args.kwargs)
        self.assertNotIn("is_active", manager.update_token.call_args.kwargs)
        self.assertEqual(manager.update_token.call_args.kwargs["google_cookies"], "validated")

    def test_new_account_does_not_create_projects(self):
        self.app.dependency_overrides[admin.verify_admin_token] = lambda: "test-session"
        database = SimpleNamespace(get_token_by_email=AsyncMock(return_value=None), add_token=AsyncMock(return_value=8))
        validated = {"success": True, "email": "test@example.com", "google_cookies": "validated", "session_token": "fixture"}
        with patch.object(admin, "db", database), patch.object(admin, "proxy_manager", None), patch.object(admin, "concurrency_manager", None), patch.object(protocol_login.protocol_loginer, "login", AsyncMock(return_value=validated)):
            response = self.client.post("/api/browser-login/sync", json=self.body)
        self.assertEqual(response.json()["added"], 1)
        self.assertIsNone(database.add_token.call_args.args[0].current_project_id)
