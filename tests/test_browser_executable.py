import os
import unittest
from unittest.mock import patch

from src.services.browser_executable import resolve_local_browser_executable


class BrowserExecutableTests(unittest.TestCase):
    @patch.dict(os.environ, {"BROWSER_EXECUTABLE_PATH": ""})
    @patch("src.services.browser_executable.sys.platform", "darwin")
    def test_macos_uses_installed_chrome_without_playwright_download(self):
        from src.services import browser_captcha

        with patch("pathlib.Path.is_file", return_value=True), patch("os.access", return_value=True), patch.object(browser_captcha, "_run_playwright_install") as download:
            self.assertEqual(resolve_local_browser_executable(), "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome")
            self.assertTrue(browser_captcha._ensure_browser_installed())
            download.assert_not_called()

    @patch.dict(os.environ, {"BROWSER_EXECUTABLE_PATH": ""})
    @patch("src.services.browser_executable.sys.platform", "darwin")
    def test_missing_chrome_fails_without_downloading(self):
        with patch("pathlib.Path.is_file", return_value=False):
            with self.assertRaisesRegex(RuntimeError, "不会自动下载"):
                resolve_local_browser_executable()

    @patch.dict(os.environ, {"BROWSER_EXECUTABLE_PATH": "/missing/browser"})
    def test_invalid_explicit_browser_fails_instead_of_falling_back(self):
        with self.assertRaisesRegex(RuntimeError, "BROWSER_EXECUTABLE_PATH"):
            resolve_local_browser_executable()

    @patch.dict(os.environ, {"BROWSER_EXECUTABLE_PATH": ""})
    @patch("src.services.browser_executable.sys.platform", "linux")
    def test_linux_preserves_existing_default(self):
        self.assertIsNone(resolve_local_browser_executable())
