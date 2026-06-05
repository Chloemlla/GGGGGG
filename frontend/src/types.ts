export type ServiceType = 'link_only' | 'link_and_bind' | 'link_and_bind_1usd';

export type TaskStatus = 'pending' | 'running' | 'completed' | 'failed' | 'cancelled';

export type AccountStatus =
  | 'pending'
  | 'running'
  | 'success'
  | 'failed'
  | 'bind_pending'
  | 'binding'
  | 'bind_success'
  | 'bind_failed'
  | 'cancelled';

export type AlertType = '' | 'success' | 'error' | 'info';

export interface Settings {
  auto_bind_enabled: boolean;
  auto_bind_one_dollar_enabled: boolean;
}

export interface VerifyCardResponse {
  valid: boolean;
  remaining: number | null;
  total_count: number | null;
  remaining_quota: string | null;
  total_quota: string | null;
  remaining_quota_units: number | null;
  total_quota_units: number | null;
  message: string;
}

export interface CardInfo {
  remaining: string | number;
  total: string | number;
  remainingUnits: number;
  totalUnits: number;
}

export interface TaskAccount {
  id: number;
  line_number: number;
  email: string;
  status: AccountStatus;
  message?: string;
  result_link?: string;
  queue_position?: number;
}

export interface TaskDetail {
  task_id: string;
  status: TaskStatus;
  total_accounts: number;
  accounts: TaskAccount[];
}

export interface TaskSummary {
  task_id: string;
  total_accounts: number;
  status: TaskStatus;
  success: number;
  failed: number;
  cancelled?: number;
  done?: number;
  created_at: string;
}

export interface ExportAccount {
  email: string;
  result_link: string;
  task_id?: string;
  line_number?: number;
}
