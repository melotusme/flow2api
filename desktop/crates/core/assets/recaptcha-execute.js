(async (siteKey, requestedAction, timeoutMs) => {
            const storageKey = '__flow2apiTrustedRecaptchaExecute';
            for (let attempt = 0; attempt < 40; attempt += 1) {
                if (window[storageKey]) break;
                await new Promise(resolve => setTimeout(resolve, 250));
            }

            const capturedExecute = !!window[storageKey];
            let execute = window[storageKey];
            let executeSource = '';
            try {
                executeSource = String(execute || '');
            } catch (error) {}

            if (!execute) {
                const enterprise = window.grecaptcha && window.grecaptcha.enterprise;
                execute = enterprise && enterprise.execute;
                try {
                    executeSource = String(execute || '');
                } catch (error) {}
                if (
                    executeSource.includes('extension_hijack_detected') &&
                    !window.__flow2apiRecaptchaActionGuardInstalled
                ) {
                    return {
                        ok: false,
                        error: 'trusted reCAPTCHA action guard is unavailable',
                        debug: {
                            hookInstalled: !!window.__flow2apiTrustedRecaptchaHookInstalled,
                            actionGuard: !!window.__flow2apiRecaptchaActionGuardInstalled,
                            captured: !!window.__flow2apiTrustedRecaptchaExecute,
                            url: location.href
                        }
                    };
                }
            }

            if (typeof execute !== 'function') {
                return { ok: false, error: 'reCAPTCHA execute is unavailable' };
            }

            let executeTarget = siteKey;
            try {
                if (
                    !capturedExecute &&
                    window.grecaptcha &&
                    window.grecaptcha.enterprise &&
                    typeof window.grecaptcha.enterprise.render === 'function' &&
                    window.__flow2apiRecaptchaClientId === undefined
                ) {
                    window.__flow2apiRecaptchaClientId =
                        window.grecaptcha.enterprise.render(
                            document.documentElement,
                            { sitekey: siteKey, action: requestedAction }
                        );
                }
                if (window.__flow2apiRecaptchaClientId !== undefined) {
                    executeTarget = window.__flow2apiRecaptchaClientId;
                }
            } catch (renderError) {
                return {
                    ok: false,
                    error: renderError && renderError.message
                        ? renderError.message
                        : String(renderError)
                };
            }

            try {
                const token = await new Promise((resolve, reject) => {
                    let settled = false;
                    const finish = (handler, value) => {
                        if (settled) return;
                        settled = true;
                        handler(value);
                    };
                    const timer = setTimeout(() => {
                        finish(reject, new Error('execute timeout'));
                    }, timeoutMs);
                    try {
                        Promise.resolve(execute(executeTarget, { action: requestedAction }))
                            .then(value => {
                                clearTimeout(timer);
                                finish(resolve, value);
                            })
                            .catch(error => {
                                clearTimeout(timer);
                                finish(reject, error);
                            });
                    } catch (error) {
                        clearTimeout(timer);
                        finish(reject, error);
                    }
                });
                return { ok: true, token };
            } catch (error) {
                const debug = {
                    captured: capturedExecute,
                    actionGuard: !!window.__flow2apiRecaptchaActionGuardInstalled,
                    renderType: window.grecaptcha && window.grecaptcha.enterprise
                        ? typeof window.grecaptcha.enterprise.render
                        : 'missing',
                    clientId: String(window.__flow2apiRecaptchaClientId),
                    executeTarget: String(executeTarget),
                    url: location.href
                };
                return {
                    ok: false,
                    error: (error && error.message ? error.message : String(error))
                        + ' | ' + JSON.stringify(debug)
                };
            }
        })(__SITE_KEY__, __REQUESTED_ACTION__, __TIMEOUT_MS__)