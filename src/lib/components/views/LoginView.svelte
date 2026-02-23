<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import TitleBar from '../TitleBar.svelte';
  import { t } from '$lib/i18n';
  import { setManualOffline } from '$lib/stores/offlineStore';
  import { qobuzTosAccepted, loadTosAcceptance, setTosAcceptance } from '$lib/stores/qobuzLegalStore';
  import { get } from 'svelte/store';

  interface UserInfo {
    userName: string;
    userId: number;
    subscription: string;
    subscriptionValidUntil?: string | null;
  }

  interface Props {
    onLoginSuccess: (userInfo: UserInfo) => void;
    onStartOffline?: () => void;
  }

  let { onLoginSuccess, onStartOffline }: Props = $props();

  let email = $state('');
  let password = $state('');
  let rememberMe = $state(true);
  let isLoading = $state(false);
  let isInitializing = $state(true);
  let initStatus = $state('Connecting to Qobuz™...');
  let error = $state<string | null>(null);
  let initError = $state<string | null>(null);
  let isTimedOut = $state(false);
  let timeoutId: ReturnType<typeof setTimeout> | null = null;

  const LOGIN_TIMEOUT_MS = 60000; // 60 seconds

  function formatErrorMessage(err: unknown): string {
    if (typeof err === 'string') return err;
    if (err instanceof Error) return err.message;
    if (err && typeof err === 'object') {
      const obj = err as Record<string, unknown>;
      const candidates = ['message', 'error', 'details', 'reason'];
      for (const key of candidates) {
        const value = obj[key];
        if (typeof value === 'string' && value.trim().length > 0) {
          return value;
        }
      }
      try {
        return JSON.stringify(obj);
      } catch {
        return 'Unknown error';
      }
    }
    return 'Unknown error';
  }

  // Initialize the Qobuz™ client on mount
  $effect(() => {
    initializeClient();
    return () => {
      // Cleanup timeout on unmount
      if (timeoutId) {
        clearTimeout(timeoutId);
        timeoutId = null;
      }
    };
  });

  // RuntimeStatus type from backend
  interface RuntimeStatus {
    state: string;
    user_id: number | null;
    client_initialized: boolean;
    legacy_auth: boolean;
    corebridge_auth: boolean;
    session_activated: boolean;
    degraded_reason: { code: string; message: string } | null;
  }

  async function initializeClient() {
    try {
      isInitializing = true;
      initError = null;
      isTimedOut = false;
      initStatus = 'Connecting to Qobuz™...';

      // Start timeout timer
      if (timeoutId) {
        clearTimeout(timeoutId);
      }
      timeoutId = setTimeout(() => {
        if (isInitializing) {
          console.warn('Login initialization timed out after 60 seconds');
          isTimedOut = true;
          isInitializing = false;
        }
      }, LOGIN_TIMEOUT_MS);

      // Load ToS acceptance first (uses localStorage fallback before session)
      initStatus = 'Loading preferences...';
      await loadTosAcceptance();

      // Use runtime_bootstrap as the SINGLE SOURCE OF TRUTH for initialization.
      // It does everything: init client, auto-login if saved creds, activate session.
      // NO legacy is_logged_in/get_user_info checks - that causes state divergence.
      initStatus = 'Initializing...';
      const status = await invoke<RuntimeStatus>('runtime_bootstrap');
      console.log('[LoginView] runtime_bootstrap result:', status);

      // Check if session is fully active (authenticated + session activated)
      if (status.session_activated && status.user_id && status.user_id > 0) {
        // Session is valid - need to get user display info
        // Use v2 command since session is active
        clearTimeoutTimer();
        try {
          const userInfo = await invoke<{ user_name: string; subscription: string; subscription_valid_until?: string | null } | null>('v2_get_user_info');
          if (userInfo) {
            console.log('[LoginView] Session restored for user_id:', status.user_id);
            onLoginSuccess({
              userName: userInfo.user_name,
              userId: status.user_id,
              subscription: userInfo.subscription,
              subscriptionValidUntil: userInfo.subscription_valid_until ?? null,
            });
            return;
          }
        } catch (err) {
          console.warn('[LoginView] Could not get user info, will show login form:', err);
        }
      }

      // Check for degraded state
      if (status.degraded_reason) {
        console.warn('[LoginView] Runtime degraded:', status.degraded_reason);
        if (status.degraded_reason.code === 'BundleExtractionFailed') {
          initError = $t('auth.connectionFailed');
          clearTimeoutTimer();
          return;
        }
      }

      // If client is initialized but no session, show login form
      if (status.client_initialized && !status.session_activated) {
        console.log('[LoginView] Client ready, no session - showing login form');
      }

      // If we reach here, no auto-login - clear timeout and show login form
      clearTimeoutTimer();
    } catch (err) {
      console.error('Failed to initialize client:', err);
      clearTimeoutTimer();
      initError = formatErrorMessage(err);
    } finally {
      if (!isTimedOut) {
        isInitializing = false;
      }
    }
  }

  function clearTimeoutTimer() {
    if (timeoutId) {
      clearTimeout(timeoutId);
      timeoutId = null;
    }
  }

  function handleRetryLogin() {
    isTimedOut = false;
    initializeClient();
  }

  async function handleLogin(e: Event) {
    e.preventDefault();

    if (!get(qobuzTosAccepted)) {
      error = $t('legal.tosRequiredToLogin');
      return;
    }

    // Persist ToS acceptance synchronously before login so the backend
    // gate in require_tos_accepted() always sees the correct value.
    // The subscribe-handler write is fire-and-forget and can lose the
    // race if the user clicks Login immediately after checking the box.
    try {
      await setTosAcceptance(true);
    } catch {
      // If persistence fails the backend will reject login — error shown below
    }

    if (!email || !password) {
      error = 'Please enter email and password';
      return;
    }

    isLoading = true;
    error = null;

    try {
      // V2 manual login with blocking CoreBridge auth
      const response = await invoke<{
        success: boolean;
        user_name?: string;
        user_id?: number;
        subscription?: string;
        subscription_valid_until?: string | null;
        error?: string;
        error_code?: string;
      }>('v2_manual_login', { email, password });

      console.log('[LoginView] v2_manual_login response:', response);

      if (response.success) {
        // Validate that we have a valid user_id - NEVER allow 0
        if (!response.user_id || response.user_id === 0) {
          console.error('[LoginView] v2_manual_login returned success but invalid user_id');
          error = $t('auth.v2AuthFailed');
          isLoading = false;
          return;
        }

        // Persist credential preference explicitly on each successful login
        if (rememberMe) {
          try {
            await invoke('v2_save_credentials', { email, password });
            console.log('Credentials saved to keyring');
          } catch (saveErr) {
            console.error('Failed to save credentials:', saveErr);
            // Don't block login if saving fails
          }
        } else {
          try {
            await invoke('v2_clear_saved_credentials');
            console.log('Saved credentials cleared (remember me disabled)');
          } catch (clearErr) {
            console.error('Failed to clear saved credentials:', clearErr);
          }
        }

        onLoginSuccess({
            userName: response.user_name || 'User',
            userId: response.user_id,
            subscription: response.subscription || 'Active',
            subscriptionValidUntil: response.subscription_valid_until ?? null,
          });
      } else {
        if (response.error_code === 'ineligible_user') {
          error = $t('auth.ineligibleSubscription');
        } else if (response.error_code === 'v2_auth_failed') {
          error = $t('auth.v2AuthFailed');
        } else if (response.error_code === 'v2_not_initialized') {
          error = $t('auth.v2NotInitialized');
        } else {
          error = response.error || 'Login failed';
        }
      }
    } catch (err) {
      console.error('Login error:', err);
      error = formatErrorMessage(err);
    } finally {
      isLoading = false;
    }
  }

  async function handleStartOffline() {
    try {
      await setManualOffline(true);
      onStartOffline?.();
    } catch (err) {
      console.error('Failed to enable offline mode:', err);
      error = formatErrorMessage(err);
    }
  }
</script>

<div class="login-wrapper">
  <TitleBar />
  <div class="login-view">
    <div class="login-card">
    <!-- Logo -->
    <div class="logo">
      <img src="/logo.png" alt="QBZ Logo" class="logo-img" />
    </div>

    {#if isTimedOut}
      <div class="timeout-box">
        <p class="timeout-title">Connection is taking too long</p>
        <p class="timeout-detail">
          Unable to connect to Qobuz™ after 60 seconds. This could be a network issue or Qobuz™ may be temporarily unavailable.
        </p>
        <div class="timeout-actions">
          <button class="retry-btn" onclick={handleRetryLogin}>Try Again</button>
          <button class="offline-btn" onclick={handleStartOffline}>Start Offline</button>
        </div>
      </div>
    {:else if isInitializing}
      <div class="initializing">
        <div class="spinner"></div>
        <p>{initStatus}</p>
      </div>
    {:else if initError}
      <div class="error-box">
        <p>Failed to connect to Qobuz™</p>
        <p class="error-detail">{initError}</p>
        <button class="retry-btn" onclick={initializeClient}>Retry</button>
      </div>
    {:else}
      <div class="login-body">
        <form onsubmit={handleLogin}>
          <div class="input-group">
            <label for="email">Email</label>
            <input
              id="email"
              type="email"
              bind:value={email}
              placeholder="your@email.com"
              disabled={isLoading}
              autocomplete="email"
            />
          </div>

          <div class="input-group">
            <label for="password">Password</label>
            <input
              id="password"
              type="password"
              bind:value={password}
              placeholder="Password"
              disabled={isLoading}
              autocomplete="current-password"
            />
          </div>

          <div class="remember-me">
            <label>
              <input type="checkbox" bind:checked={rememberMe} />
              <span>Remember me</span>
            </label>
          </div>

          <div class="remember-me tos-remember">
            <label>
              <input type="checkbox" bind:checked={$qobuzTosAccepted} disabled={isLoading} />
              <span>
                {$t('legal.tosAgreementPrefix')}
                <a href="https://www.qobuz.com/us-en/legal/terms" target="_blank" rel="noopener">
                  {$t('legal.tosLinkText')}
                </a>
              </span>
            </label>
          </div>

          {#if error}
            <div class="error-message">{error}</div>
          {/if}

          <button type="submit" class="login-btn" disabled={isLoading || !$qobuzTosAccepted}>
            {#if isLoading}
              <div class="spinner small"></div>
              <span>Signing in...</span>
            {:else}
              <span>Sign in with Qobuz™</span>
            {/if}
          </button>

          <p class="offline-link">
            <small>
              <button type="button" class="link-button" onclick={handleStartOffline}>
                {$t('offline.startWithoutLogin')}
              </button>
            </small>
          </p>
        </form>

        <div class="login-footer">
          <p class="footer-copy">
            QBZ requires an active Qobuz™ subscription. Your credentials are sent directly to Qobuz™.<br />
            This application uses the Qobuz API but is not certified by Qobuz. {$t('legal.trademarkNotice')}
          </p>
        </div>
      </div>

    {/if}
    </div>
  </div>
</div>

<style>
  .login-wrapper {
    display: flex;
    flex-direction: column;
    height: 100vh;
    background-color: var(--bg-primary);
  }

  .login-view {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    background-color: var(--bg-primary);
  }

	  .login-card {
	    width: 100%;
	    max-width: 720px;
	    padding: 52px;
	    background-color: var(--bg-secondary);
	    border-radius: 16px;
	    box-shadow: 0 8px 32px rgba(0, 0, 0, 0.4);
	    display: flex;
	    flex-direction: column;
	    min-height: min(700px, 85vh);
	    max-height: 90vh;
	    overflow-y: auto;
	  }

  .logo {
    text-align: center;
    margin-bottom: 32px;
    color: var(--accent-primary);
  }

  .logo-img {
    width: 80px;
    height: 80px;
    object-fit: contain;
  }

  .login-body {
    flex: 1;
    display: flex;
    flex-direction: column;
  }

  .login-footer {
    margin-top: auto;
    padding-top: 16px;
    text-align: center;
  }

  .tos-remember label {
    width: 100%;
  }

  .tos-remember span {
    white-space: nowrap;
  }

  .tos-remember a {
    color: var(--accent-primary);
    text-decoration: none;
  }

  .tos-remember a:hover {
    text-decoration: underline;
  }

  .initializing {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    padding: 32px 0;
  }

  .initializing p {
    margin-top: 16px;
    color: var(--text-muted);
  }

  .error-box {
    text-align: center;
    padding: 24px;
    background-color: var(--danger-bg);
    border-radius: 8px;
    margin-bottom: 16px;
  }

  .error-box p {
    color: var(--danger);
    margin-bottom: 8px;
  }

  .error-detail {
    font-size: 12px;
    color: var(--text-muted) !important;
    word-break: break-word;
  }

  .timeout-box {
    text-align: center;
    padding: 24px;
    background-color: var(--warning-bg);
    border: 1px solid var(--warning-border);
    border-radius: 8px;
    margin-bottom: 16px;
  }

  .timeout-title {
    font-size: 16px;
    font-weight: 600;
    color: var(--warning);
    margin-bottom: 12px;
  }

  .timeout-detail {
    font-size: 13px;
    color: var(--text-muted);
    line-height: 1.5;
    margin-bottom: 20px;
  }

  .timeout-actions {
    display: flex;
    gap: 12px;
    justify-content: center;
  }

  .retry-btn {
    padding: 10px 24px;
    background-color: var(--accent-primary);
    color: white;
    border: none;
    border-radius: 8px;
    cursor: pointer;
    font-size: 14px;
    font-weight: 500;
    transition: background-color 150ms ease;
  }

  .retry-btn:hover {
    background-color: var(--accent-hover);
  }

  .offline-btn {
    padding: 10px 24px;
    background-color: transparent;
    color: var(--text-secondary);
    border: 1px solid var(--text-muted);
    border-radius: 8px;
    cursor: pointer;
    font-size: 14px;
    font-weight: 500;
    transition: all 150ms ease;
  }

  .offline-btn:hover {
    border-color: var(--text-primary);
    color: var(--text-primary);
  }

  form {
    display: flex;
    flex-direction: column;
    gap: 20px;
  }

  .input-group {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .input-group label {
    font-size: 14px;
    font-weight: 500;
    color: var(--text-secondary);
  }

  .input-group input {
    height: 48px;
    padding: 0 16px;
    background-color: var(--bg-tertiary);
    border: 1px solid transparent;
    border-radius: 8px;
    font-size: 16px;
    color: var(--text-primary);
    outline: none;
    transition: border-color 150ms ease;
  }

  .input-group input:focus {
    border-color: var(--accent-primary);
  }

  .input-group input::placeholder {
    color: var(--text-muted);
  }

  .input-group input:disabled {
    opacity: 0.6;
  }

  .remember-me {
    display: flex;
    align-items: center;
  }

  .remember-me label {
    display: flex;
    align-items: center;
    gap: 8px;
    cursor: pointer;
    font-size: 14px;
    color: var(--text-secondary);
  }

  .remember-me input[type="checkbox"] {
    width: 18px;
    height: 18px;
    accent-color: var(--accent-primary);
    cursor: pointer;
  }

  .error-message {
    padding: 12px 16px;
    background-color: var(--danger-bg);
    border-radius: 8px;
    color: var(--danger);
    font-size: 14px;
  }

  .login-btn {
    height: 48px;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    background-color: var(--accent-primary);
    color: white;
    border: none;
    border-radius: 8px;
    font-size: 16px;
    font-weight: 600;
    cursor: pointer;
    transition: background-color 150ms ease;
  }

  .login-btn:hover:not(:disabled) {
    background-color: var(--accent-hover);
  }

  .login-btn:disabled {
    opacity: 0.7;
    cursor: not-allowed;
  }

  .footer-copy {
    font-size: 12px;
    color: var(--text-muted);
    text-align: center;
    line-height: 1.5;
    margin: 0;
  }

  .offline-link {
    margin-top: 4px;
    text-align: center;
  }

  .link-button {
    background: none;
    border: none;
    color: var(--text-muted);
    cursor: pointer;
    text-decoration: underline;
    font-size: inherit;
    padding: 0;
    transition: color 150ms ease;
  }

  .link-button:hover {
    color: var(--accent-primary);
  }

  .spinner {
    width: 32px;
    height: 32px;
    border: 3px solid var(--bg-tertiary);
    border-top-color: var(--accent-primary);
    border-radius: 50%;
    animation: spin 1s linear infinite;
  }

  .spinner.small {
    width: 18px;
    height: 18px;
    border-width: 2px;
    border-color: var(--alpha-30);
    border-top-color: white;
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
</style>
