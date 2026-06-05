import { useEffect, useMemo, useState, type ReactNode } from 'react';
import {
  BookOpen,
  Check,
  Clipboard,
  Download,
  ExternalLink,
  Eye,
  FileText,
  RefreshCcw,
  Search,
  Send,
  X
} from 'lucide-react';
import {
  cancelQueuedAccount,
  exportTasksByCard,
  getSettings,
  getTask,
  getTasksByCard,
  submitTask,
  verifyCard
} from './api';
import type {
  AccountStatus,
  AlertType,
  CardInfo,
  ServiceType,
  Settings,
  TaskAccount,
  TaskDetail,
  TaskStatus,
  TaskSummary,
  VerifyCardResponse
} from './types';

const PAGE_SIZE = 10;
const PAYMENT_WARNING_KEY = 'payment_warning_dismissed_date';
const FINAL_TASK_STATUSES = new Set<TaskStatus>(['completed', 'failed', 'cancelled']);
const DONE_ACCOUNT_STATUSES = new Set<AccountStatus>([
  'success',
  'failed',
  'bind_success',
  'bind_failed',
  'cancelled'
]);
const EXPORTABLE_ACCOUNT_STATUSES = new Set<AccountStatus>([
  'success',
  'bind_pending',
  'binding',
  'bind_success',
  'bind_failed'
]);

const statusLabels: Record<TaskStatus | AccountStatus, string> = {
  pending: '排队中',
  running: '运行中',
  completed: '已完成',
  failed: '失败',
  cancelled: '已取消',
  success: '成功',
  bind_pending: '待绑卡',
  binding: '绑卡中',
  bind_success: '绑卡成功',
  bind_failed: '绑卡失败'
};

const serviceOptions: Array<{
  value: ServiceType;
  label: string;
  cost: number;
  costText: string;
  settingKey?: keyof Settings;
  disabledMessage?: string;
  note?: string;
}> = [
  { value: 'link_only', label: '只提取链接', cost: 0.5, costText: '0.5' },
  {
    value: 'link_and_bind',
    label: '提取链接+手机绑卡',
    cost: 1,
    costText: '1',
    settingKey: 'auto_bind_enabled',
    disabledMessage: '目前手机绑卡暂时关闭',
    note: '已有优惠链接时仅扣0.5额度'
  },
  {
    value: 'link_and_bind_1usd',
    label: '提取链接+一刀卡绑卡',
    cost: 1.5,
    costText: '1.5',
    settingKey: 'auto_bind_one_dollar_enabled',
    disabledMessage: '目前一刀卡绑卡暂时关闭',
    note: '已有优惠链接时仅扣1额度'
  }
];

function App() {
  const [path, setPath] = useState(window.location.pathname);

  useEffect(() => {
    const onPopState = () => setPath(window.location.pathname);
    window.addEventListener('popstate', onPopState);
    return () => window.removeEventListener('popstate', onPopState);
  }, []);

  function navigate(nextPath: string) {
    window.history.pushState({}, '', nextPath);
    setPath(nextPath);
  }

  return (
    <main className="app-shell">
      {path === '/api-doc' ? <ApiDocPage onNavigate={navigate} /> : <TaskPanel onNavigate={navigate} />}
    </main>
  );
}

function TaskPanel({ onNavigate }: { onNavigate: (path: string) => void }) {
  const [settings, setSettings] = useState<Settings>({
    auto_bind_enabled: false,
    auto_bind_one_dollar_enabled: false
  });
  const [cardKey, setCardKey] = useState('');
  const [verifying, setVerifying] = useState(false);
  const [cardAlert, setCardAlert] = useState({ type: '' as AlertType, msg: '' });
  const [cardInfo, setCardInfo] = useState<CardInfo | null>(null);
  const [history, setHistory] = useState<TaskSummary[]>([]);
  const [historyMessage, setHistoryMessage] = useState('');
  const [historyLoading, setHistoryLoading] = useState(false);
  const [historyQuery, setHistoryQuery] = useState('');
  const [page, setPage] = useState(1);
  const [exportingAll, setExportingAll] = useState(false);
  const [serviceType, setServiceType] = useState<ServiceType>('link_only');
  const [accountsText, setAccountsText] = useState('');
  const [submitting, setSubmitting] = useState(false);
  const [submitAlert, setSubmitAlert] = useState({ type: '' as AlertType, msg: '' });
  const [detailOpen, setDetailOpen] = useState(false);
  const [activeTaskId, setActiveTaskId] = useState('');
  const [taskDetail, setTaskDetail] = useState<TaskDetail | null>(null);
  const [detailAlert, setDetailAlert] = useState({ type: '' as AlertType, msg: '' });
  const [cancellingId, setCancellingId] = useState<number | null>(null);
  const [dismissToday, setDismissToday] = useState(false);
  const [showPaymentWarning, setShowPaymentWarning] = useState(() => {
    try {
      return localStorage.getItem(PAYMENT_WARNING_KEY) !== todayString();
    } catch {
      return true;
    }
  });

  const resolvedServices = useMemo(
    () =>
      serviceOptions.map((option) => {
        const disabled = option.settingKey ? !settings[option.settingKey] : false;
        return {
          ...option,
          disabled,
          displayNote: disabled ? option.disabledMessage : option.note
        };
      }),
    [settings]
  );
  const selectedService = resolvedServices.find((option) => option.value === serviceType) ?? resolvedServices[0];
  const accountCount = useMemo(
    () => accountsText.split('\n').filter((line) => line.trim()).length,
    [accountsText]
  );
  const estimatedCost = useMemo(() => {
    const total = accountCount * selectedService.cost;
    return Number.isInteger(total) ? String(total) : total.toFixed(1);
  }, [accountCount, selectedService.cost]);
  const totalPages = Math.max(1, Math.ceil(history.length / PAGE_SIZE));
  const visibleHistory = history.slice((page - 1) * PAGE_SIZE, page * PAGE_SIZE);
  const hasExportableHistory = history.some((task) => task.success > 0);
  const completedCount = taskDetail?.accounts.filter((account) => DONE_ACCOUNT_STATUSES.has(account.status)).length ?? 0;
  const cancelledCount = taskDetail?.accounts.filter((account) => account.status === 'cancelled').length ?? 0;
  const progress = taskDetail?.total_accounts ? Math.round((completedCount / taskDetail.total_accounts) * 100) : 0;

  useEffect(() => {
    getSettings()
      .then(setSettings)
      .catch(() => {
        setSettings({ auto_bind_enabled: false, auto_bind_one_dollar_enabled: false });
      });
  }, []);

  useEffect(() => {
    const current = resolvedServices.find((option) => option.value === serviceType);
    if (current?.disabled) {
      setServiceType('link_only');
    }
  }, [resolvedServices, serviceType]);

  useEffect(() => {
    if (page > totalPages) {
      setPage(totalPages);
    }
  }, [page, totalPages]);

  useEffect(() => {
    if (!detailOpen || !activeTaskId) {
      return;
    }

    let alive = true;
    let intervalId: number | undefined;

    async function tick() {
      try {
        const detail = await getTask(activeTaskId);
        if (!alive) {
          return;
        }
        setTaskDetail(detail);
        updateHistoryFromDetail(detail);
        if (FINAL_TASK_STATUSES.has(detail.status) && intervalId) {
          window.clearInterval(intervalId);
        }
      } catch {
        // Keep the modal open; the user can close and retry from history.
      }
    }

    tick();
    intervalId = window.setInterval(tick, 5000);

    return () => {
      alive = false;
      if (intervalId) {
        window.clearInterval(intervalId);
      }
    };
  }, [activeTaskId, detailOpen]);

  async function loadHistory(resetPage = true) {
    const key = cardKey.trim();
    if (!key) {
      return;
    }

    setHistoryLoading(true);
    try {
      const response = await getTasksByCard(key, historyQuery.trim());
      setHistory(response.tasks || []);
      setHistoryMessage(response.message || '');
      if (resetPage) {
        setPage(1);
      }
    } catch (error) {
      setHistory([]);
      setHistoryMessage(getErrorMessage(error));
      if (resetPage) {
        setPage(1);
      }
    } finally {
      setHistoryLoading(false);
    }
  }

  async function refreshCardInfo() {
    const key = cardKey.trim();
    if (!key) {
      return;
    }

    const response = await verifyCard(key);
    if (response.valid || response.remaining !== undefined) {
      setCardInfo(normalizeCardInfo(response));
    }
  }

  async function handleVerify(resetTask = true) {
    const key = cardKey.trim();
    if (!key) {
      return;
    }

    setVerifying(true);
    setCardAlert({ type: '', msg: '' });
    if (resetTask) {
      closeDetail();
      setSubmitAlert({ type: '', msg: '' });
    }

    try {
      const response = await verifyCard(key);
      if (response.valid) {
        setCardInfo(normalizeCardInfo(response));
        setCardAlert({ type: 'success', msg: response.message });
        await loadHistory();
      } else if (response.remaining !== null && response.remaining !== undefined) {
        setCardInfo(normalizeCardInfo(response));
        setCardAlert({ type: 'info', msg: `${response.message}（可查看历史任务）` });
        await loadHistory();
      } else {
        setCardInfo(null);
        setHistory([]);
        setHistoryMessage('');
        setCardAlert({ type: 'error', msg: response.message });
      }
    } catch (error) {
      setCardInfo(null);
      setHistory([]);
      setHistoryMessage('');
      setCardAlert({ type: 'error', msg: getErrorMessage(error) });
    } finally {
      setVerifying(false);
    }
  }

  async function handleSubmit() {
    if (selectedService.disabled) {
      setSubmitAlert({ type: 'info', msg: selectedService.disabledMessage || selectedService.displayNote || '' });
      return;
    }
    if (!cardKey.trim()) {
      setSubmitAlert({ type: 'error', msg: '请先输入并验证卡密' });
      return;
    }
    if (!accountsText.trim()) {
      setSubmitAlert({ type: 'error', msg: '请输入账号信息' });
      return;
    }

    setSubmitting(true);
    setSubmitAlert({ type: '', msg: '' });
    try {
      const response = await submitTask(cardKey.trim(), accountsText, serviceType);
      setSubmitAlert({ type: 'success', msg: response.message });
      setActiveTaskId(response.task_id);
      setTaskDetail(null);
      setDetailAlert({ type: '', msg: '' });
      setDetailOpen(true);
      await refreshCardInfo();
      await loadHistory(false);
    } catch (error) {
      setSubmitAlert({ type: 'error', msg: getErrorMessage(error) });
    } finally {
      setSubmitting(false);
    }
  }

  function openTask(taskId: string) {
    setActiveTaskId(taskId);
    setTaskDetail(null);
    setDetailAlert({ type: '', msg: '' });
    setCancellingId(null);
    setDetailOpen(true);
  }

  function closeDetail() {
    setDetailOpen(false);
    setActiveTaskId('');
    setTaskDetail(null);
    setDetailAlert({ type: '', msg: '' });
    setCancellingId(null);
  }

  async function exportTask(taskId: string) {
    try {
      const detail = await getTask(taskId);
      const accounts = detail.accounts.filter(
        (account) => EXPORTABLE_ACCOUNT_STATUSES.has(account.status) && account.result_link
      );
      if (!accounts.length) {
        window.alert('该任务没有成功的账号');
        return;
      }
      downloadText(
        accounts.map((account) => `${account.email}----${account.result_link}`).join('\n'),
        `task_${taskId.slice(0, 8)}_success.txt`
      );
    } catch (error) {
      window.alert(`导出失败: ${getErrorMessage(error)}`);
    }
  }

  async function exportAll() {
    if (!cardKey.trim()) {
      window.alert('请先验证卡密');
      return;
    }
    if (!hasExportableHistory) {
      window.alert('当前历史任务没有可导出的成功账号');
      return;
    }

    setExportingAll(true);
    try {
      const response = await exportTasksByCard(cardKey.trim(), historyQuery.trim());
      if (!response.accounts.length) {
        window.alert('当前历史任务没有可导出的成功账号');
        return;
      }
      downloadText(
        response.accounts.map((account) => `${account.email}----${account.result_link}`).join('\n'),
        timestampedFileName('all_tasks_success')
      );
    } catch (error) {
      window.alert(`全部导出失败: ${getErrorMessage(error)}`);
    } finally {
      setExportingAll(false);
    }
  }

  async function cancelAccount(account: TaskAccount) {
    const label = account.email || `第 ${account.line_number} 行`;
    if (!window.confirm(`确认取消 ${label} 的排队吗？取消后会回补未消费额度。`)) {
      return;
    }

    setCancellingId(account.id);
    setDetailAlert({ type: '', msg: '' });
    try {
      const response = await cancelQueuedAccount(activeTaskId, account.id);
      setDetailAlert({ type: 'success', msg: response.message });
      await refreshCardInfo();
      const detail = await getTask(activeTaskId);
      setTaskDetail(detail);
      updateHistoryFromDetail(detail);
    } catch (error) {
      setDetailAlert({ type: 'error', msg: getErrorMessage(error) });
    } finally {
      setCancellingId(null);
    }
  }

  function updateHistoryFromDetail(detail: TaskDetail) {
    setHistory((items) =>
      items.map((item) => {
        if (item.task_id !== detail.task_id) {
          return item;
        }
        const success = detail.accounts.filter(
          (account) => account.status === 'success' || account.status === 'bind_success'
        ).length;
        const failed = detail.accounts.filter(
          (account) => account.status === 'failed' || account.status === 'bind_failed'
        ).length;
        const cancelled = detail.accounts.filter((account) => account.status === 'cancelled').length;
        const done = detail.accounts.filter((account) => DONE_ACCOUNT_STATUSES.has(account.status)).length;
        return {
          ...item,
          status: detail.status,
          total_accounts: detail.total_accounts,
          success,
          failed,
          cancelled,
          done
        };
      })
    );
  }

  function closePaymentWarning() {
    if (dismissToday) {
      try {
        localStorage.setItem(PAYMENT_WARNING_KEY, todayString());
      } catch {
        // Ignore storage errors.
      }
    }
    setShowPaymentWarning(false);
  }

  return (
    <>
      <section className="card toolbar-card">
        <div>
          <h1>操作说明</h1>
          <div className="guide-list">
            <p>1. 输入卡密并验证，确认剩余额度</p>
            <p>
              2. 在账号列表中填写账号信息，格式：
              <code>邮箱----密码----辅助邮箱----2FA密钥</code>
            </p>
            <p>3. 提交任务后等待系统自动处理，完成后会返回优惠链接</p>
            <p>4. 任务详情里可以取消仍在排队的账号，取消后会自动回补未消费额度</p>
          </div>
        </div>
        <div className="toolbar-actions">
          <button className="btn btn-primary" onClick={() => onNavigate('/api-doc')}>
            <BookOpen size={16} />
            API文档
          </button>
          <a
            className="btn btn-primary"
            href="https://my.feishu.cn/wiki/KR7hwOFmmiIq0bk7vb4cQZ7MnJb?from=from_copylink"
            target="_blank"
            rel="noopener noreferrer"
          >
            <ExternalLink size={16} />
            查看完整文档
          </a>
        </div>
      </section>

      <section className="card">
        <h2>卡密验证</h2>
        <div className="form-group">
          <label htmlFor="card-key">卡密</label>
          <div className="inline-form">
            <input
              id="card-key"
              value={cardKey}
              onChange={(event) => setCardKey(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === 'Enter') {
                  handleVerify();
                }
              }}
              placeholder="请输入卡密"
            />
            <button className="btn btn-primary" disabled={verifying} onClick={() => handleVerify()}>
              <Check size={16} />
              {verifying ? '验证中...' : '验证'}
            </button>
          </div>
        </div>
        {cardAlert.msg ? <Alert type={cardAlert.type}>{cardAlert.msg}</Alert> : null}
        {cardInfo ? (
          <div className="metric-grid">
            <div className="metric">
              <strong>{cardInfo.remaining}</strong>
              <span>剩余额度</span>
            </div>
            <div className="metric">
              <strong>{cardInfo.total}</strong>
              <span>总额度</span>
            </div>
          </div>
        ) : null}
      </section>

      {cardInfo ? (
        <section className="card">
          <h2>提交任务</h2>
          <div className="form-group">
            <label>服务类型</label>
            <div className="service-grid">
              {resolvedServices.map((option) => (
                <button
                  key={option.value}
                  className={`service-option ${serviceType === option.value ? 'selected' : ''}`}
                  aria-disabled={option.disabled}
                  onClick={() => {
                    if (!option.disabled) {
                      setServiceType(option.value);
                    }
                  }}
                >
                  <span>{option.label}</span>
                  <strong>{option.costText} 额度/账号</strong>
                  {option.displayNote ? <small>{option.displayNote}</small> : null}
                </button>
              ))}
            </div>
          </div>
          <div className="form-group">
            <label htmlFor="accounts">账号列表</label>
            <textarea
              id="accounts"
              value={accountsText}
              onChange={(event) => setAccountsText(event.target.value)}
              rows={8}
              placeholder="每行一个账号，格式：邮箱----密码----辅助邮箱----2FA密钥"
            />
            <div className="hint">
              {accountCount ? (
                <>
                  共 {accountCount} 个账号，预计最多消耗 {estimatedCost} 额度
                </>
              ) : (
                '密钥中间不要有空格'
              )}
            </div>
          </div>
          {submitAlert.msg ? <Alert type={submitAlert.type}>{submitAlert.msg}</Alert> : null}
          <button className="btn btn-success" disabled={submitting} onClick={handleSubmit}>
            <Send size={16} />
            {submitting ? '提交中...' : '提交任务'}
          </button>
        </section>
      ) : null}

      {cardKey.trim() ? (
        <section className="card">
          <div className="history-toolbar">
            <h2>历史任务</h2>
            <div className="history-actions">
              <div className="history-search">
                <input
                  value={historyQuery}
                  onChange={(event) => setHistoryQuery(event.target.value)}
                  placeholder="输入账号关键字，支持模糊搜索"
                />
                <button className="btn btn-primary" disabled={historyLoading} onClick={() => loadHistory()}>
                  <Search size={16} />
                  {historyLoading ? '查询中...' : '查询账号'}
                </button>
                <button
                  className="btn btn-ghost"
                  disabled={historyLoading || !historyQuery.trim()}
                  onClick={async () => {
                    setHistoryQuery('');
                    await loadHistory();
                  }}
                >
                  <RefreshCcw size={16} />
                  重置
                </button>
              </div>
              <button className="btn btn-success" disabled={exportingAll || !hasExportableHistory} onClick={exportAll}>
                <Download size={16} />
                {exportingAll ? '导出中...' : '全部导出'}
              </button>
            </div>
          </div>
          <div className="hint">当前按“卡密 + 账号”组合查询</div>
          {historyMessage ? <div className="hint">{historyMessage}</div> : null}
          {history.length ? (
            <>
              <div className="table-wrapper">
                <table>
                  <thead>
                    <tr>
                      <th>任务ID</th>
                      <th>账号数</th>
                      <th>成功</th>
                      <th>失败</th>
                      <th>状态</th>
                      <th>创建时间</th>
                      <th>操作</th>
                    </tr>
                  </thead>
                  <tbody>
                    {visibleHistory.map((task) => (
                      <tr key={task.task_id}>
                        <td className="mono">{task.task_id.slice(0, 8)}...</td>
                        <td>{task.total_accounts}</td>
                        <td className="good">{task.success}</td>
                        <td className="bad">{task.failed}</td>
                        <td>
                          <StatusBadge status={task.status} />
                        </td>
                        <td>{task.created_at}</td>
                        <td>
                          <div className="operation-cell">
                            <button className="btn btn-primary btn-sm" onClick={() => openTask(task.task_id)}>
                              <Eye size={14} />
                              查看
                            </button>
                            {task.success > 0 ? (
                              <button className="btn btn-success btn-sm" onClick={() => exportTask(task.task_id)}>
                                <Download size={14} />
                                导出
                              </button>
                            ) : null}
                          </div>
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
              {totalPages > 1 ? (
                <div className="pagination">
                  <button className="page-btn" disabled={page === 1} onClick={() => setPage(1)}>
                    首页
                  </button>
                  <button className="page-btn" disabled={page === 1} onClick={() => setPage(page - 1)}>
                    上一页
                  </button>
                  <span>
                    第 {page} / {totalPages} 页
                  </span>
                  <button className="page-btn" disabled={page === totalPages} onClick={() => setPage(page + 1)}>
                    下一页
                  </button>
                  <button className="page-btn" disabled={page === totalPages} onClick={() => setPage(totalPages)}>
                    末页
                  </button>
                </div>
              ) : null}
            </>
          ) : (
            <div className="empty-state">
              {historyLoading
                ? '历史任务查询中...'
                : historyQuery.trim()
                  ? '当前卡密下没有匹配该账号的历史任务'
                  : '当前卡密还没有历史任务'}
            </div>
          )}
        </section>
      ) : null}

      {detailOpen ? (
        <div className="modal-overlay" onClick={closeDetail}>
          <div className="modal-content" onClick={(event) => event.stopPropagation()}>
            <div className="modal-header">
              <h2>任务详情</h2>
              <button className="icon-btn" onClick={closeDetail} aria-label="关闭">
                <X size={18} />
              </button>
            </div>
            <div className="modal-body">
              <div className="modal-toolbar">
                <Alert type="info">任务ID: {activeTaskId}</Alert>
                <div className="hint">任务状态：{taskDetail ? statusLabels[taskDetail.status] : '-'}</div>
              </div>
              {detailAlert.msg ? <Alert type={detailAlert.type}>{detailAlert.msg}</Alert> : null}
              <div className="progress-bar">
                <div className="progress-fill" style={{ width: `${progress}%` }} />
              </div>
              <div className="hint">
                进度: {completedCount} / {taskDetail?.total_accounts ?? 0} ({progress}%)
                {cancelledCount ? `，已取消 ${cancelledCount} 个` : ''}
              </div>
              <div className="table-wrapper">
                {taskDetail?.accounts.length ? (
                  <table>
                    <thead>
                      <tr>
                        <th>#</th>
                        <th>邮箱</th>
                        <th>状态</th>
                        <th>信息</th>
                        <th>优惠链接</th>
                        <th>操作</th>
                      </tr>
                    </thead>
                    <tbody>
                      {taskDetail.accounts.map((account) => (
                        <tr key={account.id}>
                          <td>{account.line_number}</td>
                          <td>{account.email}</td>
                          <td>
                            <div className="status-meta">
                              <StatusBadge status={account.status} />
                              {account.status === 'pending' && account.queue_position ? (
                                <span className="queue-text">第{account.queue_position}位</span>
                              ) : null}
                            </div>
                          </td>
                          <td>{account.message || '-'}</td>
                          <td>
                            {account.result_link ? (
                              <a href={account.result_link} target="_blank" rel="noopener noreferrer">
                                {account.result_link}
                              </a>
                            ) : (
                              '-'
                            )}
                          </td>
                          <td>
                            {account.status === 'pending' ? (
                              <button
                                className="btn btn-danger btn-sm"
                                disabled={cancellingId !== null}
                                onClick={() => cancelAccount(account)}
                              >
                                <X size={14} />
                                {cancellingId === account.id ? '取消中...' : '取消排队'}
                              </button>
                            ) : (
                              '-'
                            )}
                          </td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                ) : (
                  <div className="empty-state">任务详情加载中...</div>
                )}
              </div>
            </div>
          </div>
        </div>
      ) : null}

      {showPaymentWarning ? (
        <div className="modal-overlay" onClick={closePaymentWarning}>
          <div className="modal-content compact-modal" onClick={(event) => event.stopPropagation()}>
            <div className="modal-header">
              <h2>温馨提示</h2>
              <button className="icon-btn" onClick={closePaymentWarning} aria-label="关闭">
                <X size={18} />
              </button>
            </div>
            <div className="warning-box">
              如果需要用 <strong>提取链接 + 绑卡</strong>，请先确保账号
              <strong>没有支付资料</strong>，否则会不成功。
              <br />
              <br />
              有支付资料请先<strong>删除支付资料</strong>再进行 <strong>提取链接 + 绑卡</strong>。
            </div>
            <label className="checkbox-row">
              <input
                type="checkbox"
                checked={dismissToday}
                onChange={(event) => setDismissToday(event.target.checked)}
              />
              今天不再弹出
            </label>
            <div className="right-actions">
              <button className="btn btn-primary" onClick={closePaymentWarning}>
                关闭
              </button>
            </div>
          </div>
        </div>
      ) : null}
    </>
  );
}

function ApiDocPage({ onNavigate }: { onNavigate: (path: string) => void }) {
  const [tab, setTab] = useState<'verify' | 'submit' | 'query' | 'list'>('verify');
  const baseUrl = window.location.origin;
  const examples = {
    verify: `curl -X POST ${baseUrl}/api/verify-card \\
  -H 'Content-Type: application/json' \\
  -d '{"card_key": "your-card-key"}'`,
    submit: `curl -X POST ${baseUrl}/api/submit-task \\
  -H 'Content-Type: application/json' \\
  -d '{"card_key": "your-card-key", "service_type": "link_only", "accounts_text": "email----password----recovery----2fakey"}'`,
    query: `curl ${baseUrl}/api/task/your-task-id`,
    list: `curl -X POST ${baseUrl}/api/tasks-by-card \\
  -H 'Content-Type: application/json' \\
  -d '{"card_key": "your-card-key"}'`
  };

  return (
    <section className="card api-doc">
      <div className="doc-header">
        <h1>API接口文档</h1>
        <button className="btn btn-primary" onClick={() => onNavigate('/')}>
          返回首页
        </button>
      </div>
      <Alert type="info">Base URL: {baseUrl}</Alert>
      <div className="tabs">
        <button className={`tab-btn ${tab === 'verify' ? 'active' : ''}`} onClick={() => setTab('verify')}>
          验证卡密
        </button>
        <button className={`tab-btn ${tab === 'submit' ? 'active' : ''}`} onClick={() => setTab('submit')}>
          提交任务
        </button>
        <button className={`tab-btn ${tab === 'query' ? 'active' : ''}`} onClick={() => setTab('query')}>
          查询任务
        </button>
        <button className={`tab-btn ${tab === 'list' ? 'active' : ''}`} onClick={() => setTab('list')}>
          任务列表
        </button>
      </div>

      {tab === 'verify' ? (
        <ApiSection
          title="1. 验证卡密"
          method="POST"
          endpoint="/api/verify-card"
          limit="20次/分钟"
          request={`{
  "card_key": "your-card-key"
}`}
          response={`{
  "valid": true,
  "remaining": 10,
  "total_count": 20,
  "remaining_quota": "5",
  "total_quota": "10",
  "remaining_quota_units": 10,
  "total_quota_units": 20,
  "message": "卡密有效"
}`}
          curl={examples.verify}
        />
      ) : null}
      {tab === 'submit' ? (
        <ApiSection
          title="2. 提交任务"
          method="POST"
          endpoint="/api/submit-task"
          limit="10次/分钟"
          request={`{
  "card_key": "your-card-key",
  "service_type": "link_only",
  "accounts_text": "email1----password1----recovery1----2fakey1\\nemail2----password2----recovery2----2fakey2"
}`}
          response={`{
  "task_id": "uuid-task-id",
  "total_accounts": 2,
  "message": "任务提交成功"
}`}
          curl={examples.submit}
          hint="service_type 可选：link_only；link_and_bind；link_and_bind_1usd"
        />
      ) : null}
      {tab === 'query' ? (
        <ApiSection
          title="3. 查询任务状态"
          method="GET"
          endpoint="/api/task/{task_id}"
          limit="60次/分钟"
          request="GET /api/task/uuid-task-id"
          response={`{
  "task_id": "uuid-task-id",
  "status": "running",
  "total_accounts": 2,
  "accounts": [
    {
      "id": 1,
      "line_number": 1,
      "email": "test@example.com",
      "status": "success",
      "message": "处理成功",
      "result_link": "https://example.com/link"
    }
  ]
}`}
          curl={examples.query}
        />
      ) : null}
      {tab === 'list' ? (
        <ApiSection
          title="4. 查询卡密的所有任务"
          method="POST"
          endpoint="/api/tasks-by-card"
          limit="20次/分钟"
          request={`{
  "card_key": "your-card-key"
}`}
          response={`{
  "tasks": [
    {
      "task_id": "uuid-task-id",
      "total_accounts": 2,
      "status": "completed",
      "success": 2,
      "failed": 0,
      "created_at": "2026-03-28 10:00:00"
    }
  ],
  "message": "查询成功"
}`}
          curl={examples.list}
        />
      ) : null}
    </section>
  );
}

function ApiSection({
  title,
  method,
  endpoint,
  limit,
  request,
  response,
  curl,
  hint
}: {
  title: string;
  method: 'GET' | 'POST';
  endpoint: string;
  limit: string;
  request: string;
  response: string;
  curl: string;
  hint?: string;
}) {
  return (
    <div className="api-section">
      <h2>{title}</h2>
      <div className="api-info">
        <span className={`method ${method === 'GET' ? 'get' : ''}`}>{method}</span>
        <span className="endpoint">{endpoint}</span>
        <span className="limit">限制: {limit}</span>
      </div>
      <CodeBlock title="请求示例" value={request} copyValue={curl} />
      {hint ? <div className="hint doc-hint">{hint}</div> : null}
      <CodeBlock title="响应示例" value={response} />
    </div>
  );
}

function CodeBlock({ title, value, copyValue }: { title: string; value: string; copyValue?: string }) {
  async function copy() {
    await navigator.clipboard.writeText(copyValue || value);
    window.alert('已复制到剪贴板');
  }

  return (
    <div className="code-block">
      <div className="code-header">
        <span>{title}</span>
        <button className="copy-btn" onClick={copy}>
          <Clipboard size={14} />
          复制
        </button>
      </div>
      <pre>
        <code>{value}</code>
      </pre>
    </div>
  );
}

function Alert({ type, children }: { type: AlertType; children: ReactNode }) {
  return <div className={`alert alert-${type || 'info'}`}>{children}</div>;
}

function StatusBadge({ status }: { status: TaskStatus | AccountStatus }) {
  return <span className={`status-badge status-${status}`}>{statusLabels[status] || status}</span>;
}

function normalizeCardInfo(response: VerifyCardResponse): CardInfo {
  return {
    remaining: response.remaining_quota ?? response.remaining ?? 0,
    total: response.total_quota ?? response.total_count ?? 0,
    remainingUnits: response.remaining_quota_units ?? response.remaining ?? 0,
    totalUnits: response.total_quota_units ?? response.total_count ?? 0
  };
}

function getErrorMessage(error: unknown) {
  return error instanceof Error ? error.message : '请求失败';
}

function todayString() {
  const now = new Date();
  const pad = (value: number) => String(value).padStart(2, '0');
  return `${now.getFullYear()}-${pad(now.getMonth() + 1)}-${pad(now.getDate())}`;
}

function timestampedFileName(prefix: string) {
  const now = new Date();
  const pad = (value: number) => String(value).padStart(2, '0');
  return `${prefix}_${now.getFullYear()}${pad(now.getMonth() + 1)}${pad(now.getDate())}_${pad(now.getHours())}${pad(
    now.getMinutes()
  )}${pad(now.getSeconds())}.txt`;
}

function downloadText(text: string, fileName: string) {
  const blob = new Blob([text], { type: 'text/plain;charset=utf-8' });
  const url = URL.createObjectURL(blob);
  const link = document.createElement('a');
  link.href = url;
  link.download = fileName;
  link.click();
  URL.revokeObjectURL(url);
}

export default App;
