console.log("[Flow2API] Extension Content script ready.");

function detectEmailFromDOM() {
    for (const el of document.querySelectorAll('[aria-label]')) {
        const label = el.getAttribute('aria-label') || '';
        const match = label.match(/\(([^()\s]+@[^()\s]+\.[^()\s]+)\)/);
        if (match) return match[1].toLowerCase();
    }
    const text = document.body?.innerText || "";
    const candidates = [...text.matchAll(/[A-Z0-9._%+-]+@[A-Z0-9.-]+\.[A-Z]{2,}/gi)]
        .map(match => match[0].toLowerCase());
    return candidates[0] || null;
}

chrome.runtime.onMessage.addListener((message, sender, sendResponse) => {
    if (message.type === "flow2api_detect_email") {
        sendResponse({ email: detectEmailFromDOM() });
        return false;
    }
});
