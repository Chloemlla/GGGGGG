import type {
  AdminCdkMapping,
  AdminAuthStatus,
  CdkUsageLog,
  ExportAccount,
  ServiceType,
  Settings,
  TaskDetail,
  TaskSummary,
  VerifyCardResponse
} from './types';

const API_BASE = resolveApiBase();

export class ApiRequestError extends Error {
  status: number;

  constructor(message: string, status: number) {
    super(message);
    this.name = 'ApiRequestError';
    this.status = status;
  }
}

function resolveApiBase() {
  const configured = import.meta.env.VITE_API_BASE?.trim();
  if (configured) {
    return configured.replace(/\/$/, '');
  }

  if (typeof window !== 'undefined' && window.location.protocol !== 'file:') {
    return window.location.origin;
  }

  return '';
}

async function request<T>(path: string, init: RequestInit = {}): Promise<T> {
  const response = await fetch(`${API_BASE}${path}`, {
    credentials: 'include',
    headers: {
      'Content-Type': 'application/json',
      ...init.headers
    },
    ...init
  });

  const text = await response.text();
  const data = text ? JSON.parse(text) : {};

  if (!response.ok) {
    throw new ApiRequestError(data.detail || '请求失败', response.status);
  }

  return data as T;
}

export function apiUrl(path: string) {
  if (/^https?:\/\//i.test(path)) {
    return path;
  }

  return `${API_BASE}${path}`;
}

export function isAuthError(error: unknown) {
  return error instanceof ApiRequestError && (error.status === 401 || error.status === 403);
}

export function getSettings() {
  return request<Settings>('/api/settings');
}

export function verifyCard(cardKey: string) {
  return request<VerifyCardResponse>('/api/verify-card', {
    method: 'POST',
    body: JSON.stringify({ card_key: cardKey })
  });
}

export function submitTask(cardKey: string, accountsText: string, serviceType: ServiceType) {
  return request<{ task_id: string; total_accounts: number; message: string }>('/api/submit-task', {
    method: 'POST',
    body: JSON.stringify({
      card_key: cardKey,
      accounts_text: accountsText,
      service_type: serviceType
    })
  });
}

export function getTask(taskId: string) {
  return request<TaskDetail>(`/api/task/${taskId}`);
}

export function getTasksByCard(cardKey: string, accountQuery = '') {
  return request<{ tasks: TaskSummary[]; message: string }>('/api/tasks-by-card', {
    method: 'POST',
    body: JSON.stringify({
      card_key: cardKey,
      account_query: accountQuery
    })
  });
}

export function exportTasksByCard(cardKey: string, accountQuery = '') {
  return request<{ accounts: ExportAccount[]; message: string }>('/api/tasks/export-by-card', {
    method: 'POST',
    body: JSON.stringify({
      card_key: cardKey,
      account_query: accountQuery
    })
  });
}

export function cancelQueuedAccount(taskId: string, accountId: number) {
  return request<{ message: string }>(`/api/task/${taskId}/account/${accountId}/cancel-queue`, {
    method: 'POST'
  });
}

export function listAdminCdks() {
  return request<{ items: AdminCdkMapping[] }>('/api/admin/cdks');
}

export function createAdminCdk(payload: {
  distribution_cdk?: string;
  upstream_cdk: string;
  note?: string;
}) {
  return request<AdminCdkMapping>('/api/admin/cdks', {
    method: 'POST',
    body: JSON.stringify(payload)
  });
}

export function deleteAdminCdk(id: string) {
  return request<{ message: string }>(`/api/admin/cdks/${id}`, {
    method: 'DELETE'
  });
}

export function listAdminCdkUsage(params: { distribution_cdk?: string; limit?: number } = {}) {
  const query = new URLSearchParams();
  if (params.distribution_cdk?.trim()) {
    query.set('distribution_cdk', params.distribution_cdk.trim());
  }
  if (params.limit) {
    query.set('limit', String(params.limit));
  }

  const suffix = query.toString() ? `?${query.toString()}` : '';
  return request<{ items: CdkUsageLog[] }>(`/api/admin/cdk-usage${suffix}`);
}

export function getAdminCdkUsage(id: string) {
  return request<CdkUsageLog>(`/api/admin/cdk-usage/${id}`);
}

export function getAdminAuthStatus() {
  return request<AdminAuthStatus>('/api/admin/auth/status');
}

export function logoutAdmin() {
  return request<{ message: string }>('/api/admin/auth/logout', {
    method: 'POST'
  });
}
