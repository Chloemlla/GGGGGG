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

export interface AdminCdkMapping {
  id: string;
  distribution_cdk: string;
  upstream_cdk_masked: string;
  note?: string;
  enabled: boolean;
  created_at: string;
  updated_at: string;
}

export interface CdkUsageLog {
  id: string;
  request_id: string;
  distribution_cdk?: string | null;
  requested_cdk_masked: string;
  mapping_id?: string | null;
  cdk_note?: string | null;
  matched_mapping: boolean;
  request_method: string;
  request_path: string;
  request_query?: string | null;
  client_ip?: string | null;
  forwarded_for?: string | null;
  user_agent?: string | null;
  referer?: string | null;
  origin?: string | null;
  accept_language?: string | null;
  service_type?: string | null;
  account_count?: number | null;
  request_body_bytes: number;
  request_summary?: string | null;
  response_status?: number | null;
  response_body_bytes?: number | null;
  response_summary?: string | null;
  error?: string | null;
  duration_ms: number;
  created_at: string;
}

export interface SynapseAdminUser {
  sub?: string;
  id?: string;
  username?: string;
  name?: string;
  avatarUrl?: string;
  role?: string;
  roles?: string[];
  admin?: boolean;
  isAdmin?: boolean;
  is_admin?: boolean;
  synapseAdmin?: boolean;
  synapse_admin?: boolean;
  isTrusted?: boolean;
  is_trusted?: boolean;
  authProvider?: string;
  createdAt?: string;
  accountStatus?: string;
  email?: string;
  emailVerified?: boolean;
}

export interface AdminAuthStatus {
  authenticated: boolean;
  configured: boolean;
  login_url: string;
  message?: string | null;
  user?: SynapseAdminUser | null;
}
