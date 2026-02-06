/**
 * Authentication State Store
 *
 * Manages user authentication state and user info.
 */

export interface UserInfo {
  userName: string;
  userId: number;
  subscription: string;
  subscriptionValidUntil?: string | null;
}

// Auth state
let isLoggedIn = false;
let userInfo: UserInfo | null = null;

// Listeners
const listeners = new Set<() => void>();

function notifyListeners(): void {
  for (const listener of listeners) {
    listener();
  }
}

/**
 * Subscribe to auth state changes
 */
export function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  listener(); // Immediately notify with current state
  return () => listeners.delete(listener);
}

// ============ Getters ============

export function getIsLoggedIn(): boolean {
  return isLoggedIn;
}

export function getUserInfo(): UserInfo | null {
  return userInfo;
}

// ============ Actions ============

/**
 * Set login state after successful authentication
 */
export function setLoggedIn(info: UserInfo): void {
  isLoggedIn = true;
  userInfo = info;
  notifyListeners();
}

/**
 * Clear auth state on logout
 */
export function setLoggedOut(): void {
  isLoggedIn = false;
  userInfo = null;
  notifyListeners();
}

// ============ State Getter ============

export interface AuthState {
  isLoggedIn: boolean;
  userInfo: UserInfo | null;
}

export function getAuthState(): AuthState {
  return {
    isLoggedIn,
    userInfo
  };
}
