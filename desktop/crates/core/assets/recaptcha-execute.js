(async (siteKey, requestedAction, timeoutMs) => {
            const storageKey = '__flow2apiTrustedRecaptchaExecute';
            const deadline = Date.now() + Math.max(0, timeoutMs);
            const captured = window[storageKey];
            const enterprise = window.grecaptcha && window.grecaptcha.enterprise;
            const debug = {
                hookInstalled: !!window.__flow2apiTrustedRecaptchaHookInstalled,
                actionGuard: !!window.__flow2apiRecaptchaActionGuardInstalled,
                captured: typeof captured === 'function',
                executeType: enterprise ? typeof enterprise.execute : 'missing',
                renderType: enterprise ? typeof enterprise.render : 'missing'
            };
            const errors = [];
            const strategies = [];
            let container = null;

            if (typeof captured === 'function') {
                strategies.push(['captured', () => captured(siteKey, {action: requestedAction})]);
            }
            if (enterprise && typeof enterprise.execute === 'function') {
                let executeSource = '';
                try { executeSource = String(enterprise.execute); } catch (error) {}
                if (
                    executeSource.includes('extension_hijack_detected') &&
                    !debug.actionGuard
                ) {
                    // Both public paths use the same wrapper and require the guard.
                    errors.push('trusted reCAPTCHA action guard is unavailable');
                } else {
                    strategies.push(['direct', () => enterprise.execute.call(
                        enterprise, siteKey, {action: requestedAction}
                    )]);
                    if (typeof enterprise.render === 'function') {
                        strategies.push(['render', () => {
                            container = document.createElement('div');
                            container.setAttribute('data-flow2api-recaptcha', '1');
                            container.style.display = 'none';
                            (document.body || document.documentElement).appendChild(container);
                            const clientId = enterprise.render(
                                container, {sitekey: siteKey, size: 'invisible', action: requestedAction}
                            );
                            return enterprise.execute.call(
                                enterprise, clientId, {action: requestedAction}
                            );
                        }]);
                    }
                }
            }

            const attempts = [];
            let successfulToken = null;
            let resolveSuccess;
            const successPromise = new Promise(resolve => { resolveSuccess = resolve; });
            const waitForResult = async (completion, waitMs) => {
                let timer;
                try {
                    await Promise.race([
                        successPromise,
                        completion,
                        new Promise(resolve => {
                            timer = setTimeout(resolve, Math.max(0, waitMs));
                        })
                    ]);
                } finally {
                    clearTimeout(timer);
                }
            };

            try {
                for (let index = 0; index < strategies.length; index += 1) {
                    const [label, factory] = strategies[index];
                    const remaining = deadline - Date.now();
                    if (remaining <= 0) {
                        errors.push(label + ': total execute timeout');
                        break;
                    }
                    const attempt = {label, settled: false};
                    attempt.promise = Promise.resolve().then(factory).then(token => {
                        attempt.settled = true;
                        const normalized = typeof token === 'string' ? token.trim() : '';
                        if (!normalized) {
                            errors.push(label + ': empty token');
                        } else if (Date.now() >= deadline) {
                            errors.push(label + ': total execute timeout');
                        } else if (!successfulToken) {
                            successfulToken = normalized;
                            resolveSuccess(normalized);
                        }
                    }, error => {
                        attempt.settled = true;
                        errors.push(label + ': ' + (error && error.message ? error.message : String(error)));
                    });
                    attempts.push(attempt);
                    // Launch fallbacks without discarding earlier pending results.
                    const attemptMs = Math.max(1, Math.floor(remaining / (strategies.length - index)));
                    await waitForResult(attempt.promise, attemptMs);
                    if (successfulToken) return {ok: true, token: successfulToken, debug};
                }
                const remaining = deadline - Date.now();
                if (remaining > 0 && attempts.some(attempt => !attempt.settled)) {
                    await waitForResult(
                        Promise.all(attempts.map(attempt => attempt.promise)), remaining
                    );
                }
                if (successfulToken) return {ok: true, token: successfulToken, debug};
                for (const attempt of attempts) {
                    if (!attempt.settled) errors.push(attempt.label + ': total execute timeout');
                }
                return {
                    ok: false,
                    error: errors.join(' | ') || 'reCAPTCHA execute is unavailable',
                    debug
                };
            } finally {
                if (container) container.remove();
            }
        })(__SITE_KEY__, __REQUESTED_ACTION__, __TIMEOUT_MS__)
