import type {
  ExportAccount,
  ServiceType,
  Settings,
  TaskDetail,
  TaskSummary,
  VerifyCardResponse
} from './types';

const API_BASE = import.meta.env.VITE_API_BASE ?? '';

async function request<T>(path: string, init: RequestInit = {}): Promise<T> {
  const response = await fetch(`${API_BASE}${path}`, {
    headers: {
      'Content-Type': 'application/json',
      ...init.headers
    },
    ...init
  });

  const text = await response.text();
  const data = text ? JSON.parse(text) : {};

  if (!response.ok) {
    throw new Error(data.detail || '请求失败');
  }

  return data as T;
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
