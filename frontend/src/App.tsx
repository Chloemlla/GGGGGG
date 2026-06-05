import { useEffect, useMemo, useState, type FormEvent, type ReactNode } from 'react';
import {
  Check,
  Copy,
  Database,
  Download,
  Eye,
  Home,
  KeyRound,
  Plus,
  RefreshCcw,
  Search,
  Send,
  Shield,
  Trash2,
  X
} from 'lucide-react';
import {
  cancelQueuedAccount,
  createAdminCdk,
  deleteAdminCdk,
  exportTasksByCard,
  getSettings,
  getTask,
  getTasksByCard,
  listAdminCdks,
  submitTask,
  verifyCard
} from './api';
import type {
  AccountStatus,
  AdminCdkMapping,
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
    label: '提取链接 + 手机绑卡',
    cost: 1,
    costText: '1',
    settingKey: 'auto_bind_enabled',
    disabledMessage: '目前手机绑卡暂时关闭',
    note: '已有优惠链接时仅扣 0.5 额度'
  },
  {
    value: 'link_and_bind_1usd',
    label: '提取链接 + 一刀卡绑卡',
    cost: 1.5,
    costText: '1.5',
    settingKey: 'auto_bind_one_dollar_enabled',
    disabledMessage: '目前一刀卡绑卡暂时关闭',
    note: '已有优惠链接时仅扣 1 额度'
  }
];

type RouteName = 'workspace' | 'admin';

function App() {
  const [route, setRoute] = useState<RouteName>(() => routeFromPath(window.location.pathname));

  useEffect(() => {
    const handlePopState = () => setRoute(routeFromPath(window.location.pathname));
    window.addEventListener('popstate', handlePopState);
    return () => window.removeEventListener('popstate', handlePopState);
  }, []);

  function navigate(nextRoute: RouteName) {
    const path = nextRoute === 'admin' ? '/admin' : '/';
    window.history.pushState({}, '', path);
    setRoute(nextRoute);
  }

  return (
    <div className="app-frame">
      <BackgroundParticles />
      <a className="skip-link" href="#app-main-content">
        跳到主要内容
      </a>
      <nav className="app-nav" aria-label="主导航">
        <div className="nav-inner">
          <button className="brand-link" onClick={() => navigate('workspace')} aria-label="返回工作台">
            <span className="brand-mark">
              <Shield size={18} />
            </span>
            <span>Synapse</span>
          </button>
          <div className="nav-actions">
            <button
              className={`nav-button ${route === 'workspace' ? 'active' : ''}`}
              onClick={() => navigate('workspace')}
              aria-current={route === 'workspace' ? 'page' : undefined}
            >
              <Home size={16} />
              工作台
            </button>
            <button
              className={`nav-button ${route === 'admin' ? 'active' : ''}`}
              onClick={() => navigate('admin')}
              aria-current={route === 'admin' ? 'page' : undefined}
            >
              <KeyRound size={16} />
              管理员
            </button>
          </div>
        </div>
      </nav>
      <main id="app-main-content" tabIndex={-1} className="app-main">
        {route === 'admin' ? <AdminPage /> : <TaskPanel />}
      </main>
      <footer className="app-footer">
        <span>Base URL: https://pixel.yh-mo.xyz</span>
        <span>MongoDB 持久化分发 CDK</span>
      </footer>
    </div>
  );
}

function TaskPanel() {
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
    if (!detailOpen && !showPaymentWarning) {
      return;
    }

    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        if (detailOpen) {
          closeDetail();
        }
        if (showPaymentWarning) {
          closePaymentWarning();
        }
      }
    };
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [detailOpen, showPaymentWarning]);

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
    <section className="page-stack" aria-labelledby="workspace-title">
      <PageHeader
        eyebrow="Discount Link Console"
        title="自助提取优惠链接系统"
        description="输入上游 CDK 或管理员分发 CDK，后端会统一代理到 pixel.yh-mo.xyz 并保持任务状态可追踪。"
      />

      <div className="workspace-grid">
        <section className="glass-panel">
          <PanelTitle icon={<KeyRound size={18} />} title="卡密验证" description="验证额度并载入当前卡密历史任务。" />
          <div className="form-group">
            <label htmlFor="card-key">卡密 / 分发 CDK</label>
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
                placeholder="请输入卡密或管理员分发 CDK"
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
              <Metric label="剩余额度" value={cardInfo.remaining} />
              <Metric label="总额度" value={cardInfo.total} />
            </div>
          ) : null}
        </section>

        <section className="glass-panel">
          <PanelTitle icon={<Send size={18} />} title="提交任务" description="每行一个账号，提交后自动轮询任务详情。" />
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
              {accountCount ? `共 ${accountCount} 个账号，预计最多消耗 ${estimatedCost} 额度` : '密钥中间不要有空格'}
            </div>
          </div>
          {submitAlert.msg ? <Alert type={submitAlert.type}>{submitAlert.msg}</Alert> : null}
          <button className="btn btn-primary" disabled={submitting} onClick={handleSubmit}>
            <Send size={16} />
            {submitting ? '提交中...' : '提交任务'}
          </button>
        </section>
      </div>

      <section className="glass-panel">
        <div className="panel-toolbar">
          <PanelTitle icon={<Database size={18} />} title="历史任务" description="按卡密与账号关键字组合查询。" />
          <div className="history-actions">
            <div className="history-search">
              <input
                value={historyQuery}
                onChange={(event) => setHistoryQuery(event.target.value)}
                placeholder="输入账号关键字，支持模糊搜索"
              />
              <button className="btn btn-secondary" disabled={historyLoading || !cardKey.trim()} onClick={() => loadHistory()}>
                <Search size={16} />
                {historyLoading ? '查询中...' : '查询账号'}
              </button>
              <button
                className="btn btn-secondary"
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
            <button className="btn btn-dark" disabled={exportingAll || !hasExportableHistory} onClick={exportAll}>
              <Download size={16} />
              {exportingAll ? '导出中...' : '全部导出'}
            </button>
          </div>
        </div>
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
                          <button className="btn btn-secondary btn-sm" onClick={() => openTask(task.task_id)}>
                            <Eye size={14} />
                            查看
                          </button>
                          {task.success > 0 ? (
                            <button className="btn btn-dark btn-sm" onClick={() => exportTask(task.task_id)}>
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

      {detailOpen ? (
        <TaskDetailModal
          taskId={activeTaskId}
          detail={taskDetail}
          alert={detailAlert}
          completedCount={completedCount}
          cancelledCount={cancelledCount}
          progress={progress}
          cancellingId={cancellingId}
          onCancelAccount={cancelAccount}
          onClose={closeDetail}
        />
      ) : null}

      {showPaymentWarning ? (
        <PaymentWarningModal
          dismissToday={dismissToday}
          onDismissTodayChange={setDismissToday}
          onClose={closePaymentWarning}
        />
      ) : null}
    </section>
  );
}

function AdminPage() {
  const [items, setItems] = useState<AdminCdkMapping[]>([]);
  const [distributionCdk, setDistributionCdk] = useState('');
  const [upstreamCdk, setUpstreamCdk] = useState('');
  const [note, setNote] = useState('');
  const [loading, setLoading] = useState(false);
  const [creating, setCreating] = useState(false);
  const [deletingId, setDeletingId] = useState('');
  const [alert, setAlert] = useState({ type: '' as AlertType, msg: '' });

  useEffect(() => {
    loadCdks();
  }, []);

  async function loadCdks() {
    setLoading(true);
    try {
      const response = await listAdminCdks();
      setItems(response.items);
    } catch (error) {
      setAlert({ type: 'error', msg: getErrorMessage(error) });
    } finally {
      setLoading(false);
    }
  }

  async function handleCreate(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!upstreamCdk.trim()) {
      setAlert({ type: 'error', msg: '请输入上游 CDK' });
      return;
    }

    setCreating(true);
    setAlert({ type: '', msg: '' });
    try {
      const created = await createAdminCdk({
        distribution_cdk: distributionCdk.trim() || undefined,
        upstream_cdk: upstreamCdk.trim(),
        note: note.trim() || undefined
      });
      setItems((current) => [created, ...current]);
      setDistributionCdk('');
      setUpstreamCdk('');
      setNote('');
      setAlert({ type: 'success', msg: '分发 CDK 已创建' });
    } catch (error) {
      setAlert({ type: 'error', msg: getErrorMessage(error) });
    } finally {
      setCreating(false);
    }
  }

  async function handleDelete(item: AdminCdkMapping) {
    if (!window.confirm(`确认删除分发 CDK ${item.distribution_cdk}？`)) {
      return;
    }

    setDeletingId(item.id);
    try {
      await deleteAdminCdk(item.id);
      setItems((current) => current.filter((candidate) => candidate.id !== item.id));
      setAlert({ type: 'success', msg: '分发 CDK 已删除' });
    } catch (error) {
      setAlert({ type: 'error', msg: getErrorMessage(error) });
    } finally {
      setDeletingId('');
    }
  }

  async function copyCdk(value: string) {
    await navigator.clipboard.writeText(value);
    setAlert({ type: 'info', msg: '分发 CDK 已复制' });
  }

  return (
    <section className="page-stack" aria-labelledby="admin-title">
      <PageHeader
        eyebrow="Admin Distribution"
        title="分发 CDK 管理"
        description="为用户创建可分发的 CDK，并在后端 MongoDB 中绑定真实上游 CDK。用户请求会自动替换后转发到 pixel.yh-mo.xyz。"
      />

      <div className="admin-grid">
        <section className="glass-panel">
          <PanelTitle icon={<Plus size={18} />} title="创建分发 CDK" description="留空分发 CDK 时系统会自动生成。" />
          <form className="form-stack" onSubmit={handleCreate}>
            <div className="form-group">
              <label htmlFor="distribution-cdk">分发 CDK</label>
              <input
                id="distribution-cdk"
                value={distributionCdk}
                onChange={(event) => setDistributionCdk(event.target.value)}
                placeholder="可选，例如 promo-user-001"
              />
            </div>
            <div className="form-group">
              <label htmlFor="upstream-cdk">上游 CDK</label>
              <input
                id="upstream-cdk"
                value={upstreamCdk}
                onChange={(event) => setUpstreamCdk(event.target.value)}
                placeholder="必填，真实 pixel.yh-mo.xyz 卡密"
              />
            </div>
            <div className="form-group">
              <label htmlFor="cdk-note">备注</label>
              <input
                id="cdk-note"
                value={note}
                onChange={(event) => setNote(event.target.value)}
                placeholder="可选，用于区分来源或客户"
              />
            </div>
            {alert.msg ? <Alert type={alert.type}>{alert.msg}</Alert> : null}
            <button className="btn btn-primary" disabled={creating} type="submit">
              <Plus size={16} />
              {creating ? '创建中...' : '创建分发 CDK'}
            </button>
          </form>
        </section>

        <section className="glass-panel info-panel">
          <PanelTitle icon={<Database size={18} />} title="存储策略" description="MongoDB collection: cdk_mappings" />
          <div className="info-list">
            <div>
              <strong>分发 CDK</strong>
              <span>用户可见，用于主工作台验证和提交任务。</span>
            </div>
            <div>
              <strong>上游 CDK</strong>
              <span>只保存在后端，列表中仅展示脱敏值。</span>
            </div>
            <div>
              <strong>代理策略</strong>
              <span>请求体中的 card_key 命中分发 CDK 时替换为上游 CDK。</span>
            </div>
          </div>
        </section>
      </div>

      <section className="glass-panel">
        <div className="panel-toolbar">
          <PanelTitle icon={<KeyRound size={18} />} title="已创建 CDK" description="删除后该分发 CDK 将无法继续映射上游。" />
          <button className="btn btn-secondary" disabled={loading} onClick={loadCdks}>
            <RefreshCcw size={16} />
            {loading ? '刷新中...' : '刷新'}
          </button>
        </div>
        {items.length ? (
          <div className="table-wrapper">
            <table>
              <thead>
                <tr>
                  <th>分发 CDK</th>
                  <th>上游 CDK</th>
                  <th>备注</th>
                  <th>状态</th>
                  <th>创建时间</th>
                  <th>操作</th>
                </tr>
              </thead>
              <tbody>
                {items.map((item) => (
                  <tr key={item.id}>
                    <td className="mono">{item.distribution_cdk}</td>
                    <td className="mono">{item.upstream_cdk_masked}</td>
                    <td>{item.note || '-'}</td>
                    <td>
                      <span className={`status-badge ${item.enabled ? 'status-success' : 'status-cancelled'}`}>
                        {item.enabled ? '启用' : '停用'}
                      </span>
                    </td>
                    <td>{formatTimestamp(item.created_at)}</td>
                    <td>
                      <div className="operation-cell">
                        <button className="btn btn-secondary btn-sm" onClick={() => copyCdk(item.distribution_cdk)}>
                          <Copy size={14} />
                          复制
                        </button>
                        <button
                          className="btn btn-danger btn-sm"
                          disabled={deletingId === item.id}
                          onClick={() => handleDelete(item)}
                        >
                          <Trash2 size={14} />
                          {deletingId === item.id ? '删除中...' : '删除'}
                        </button>
                      </div>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        ) : (
          <div className="empty-state">{loading ? 'CDK 列表加载中...' : '还没有分发 CDK'}</div>
        )}
      </section>
    </section>
  );
}

function TaskDetailModal({
  taskId,
  detail,
  alert,
  completedCount,
  cancelledCount,
  progress,
  cancellingId,
  onCancelAccount,
  onClose
}: {
  taskId: string;
  detail: TaskDetail | null;
  alert: { type: AlertType; msg: string };
  completedCount: number;
  cancelledCount: number;
  progress: number;
  cancellingId: number | null;
  onCancelAccount: (account: TaskAccount) => void;
  onClose: () => void;
}) {
  return (
    <div className="modal-overlay" onClick={onClose}>
      <div
        className="modal-content wide-modal"
        role="dialog"
        aria-modal="true"
        aria-labelledby="task-detail-title"
        onClick={(event) => event.stopPropagation()}
      >
        <div className="modal-header">
          <div>
            <p className="eyebrow">Task Detail</p>
            <h2 id="task-detail-title">任务详情</h2>
          </div>
          <button className="icon-btn" onClick={onClose} aria-label="关闭任务详情">
            <X size={18} />
          </button>
        </div>
        <div className="modal-body">
          <div className="modal-toolbar">
            <Alert type="info">任务ID: {taskId}</Alert>
            <div className="hint">任务状态：{detail ? statusLabels[detail.status] : '-'}</div>
          </div>
          {alert.msg ? <Alert type={alert.type}>{alert.msg}</Alert> : null}
          <div className="progress-bar">
            <div className="progress-fill" style={{ width: `${progress}%` }} />
          </div>
          <div className="hint">
            进度: {completedCount} / {detail?.total_accounts ?? 0} ({progress}%)
            {cancelledCount ? `，已取消 ${cancelledCount} 个` : ''}
          </div>
          <div className="table-wrapper">
            {detail?.accounts.length ? (
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
                  {detail.accounts.map((account) => (
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
                            onClick={() => onCancelAccount(account)}
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
  );
}

function PaymentWarningModal({
  dismissToday,
  onDismissTodayChange,
  onClose
}: {
  dismissToday: boolean;
  onDismissTodayChange: (value: boolean) => void;
  onClose: () => void;
}) {
  return (
    <div className="modal-overlay" onClick={onClose}>
      <div
        className="modal-content compact-modal"
        role="dialog"
        aria-modal="true"
        aria-labelledby="payment-warning-title"
        onClick={(event) => event.stopPropagation()}
      >
        <div className="modal-header">
          <div>
            <p className="eyebrow">Notice</p>
            <h2 id="payment-warning-title">温馨提示</h2>
          </div>
          <button className="icon-btn" onClick={onClose} aria-label="关闭温馨提示">
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
            onChange={(event) => onDismissTodayChange(event.target.checked)}
          />
          今天不再弹出
        </label>
        <div className="right-actions">
          <button className="btn btn-primary" onClick={onClose}>
            关闭
          </button>
        </div>
      </div>
    </div>
  );
}

function PageHeader({ eyebrow, title, description }: { eyebrow: string; title: string; description: string }) {
  return (
    <header className="page-header">
      <p className="eyebrow">{eyebrow}</p>
      <h1 id={title.includes('管理') ? 'admin-title' : 'workspace-title'}>{title}</h1>
      <p>{description}</p>
    </header>
  );
}

function PanelTitle({ icon, title, description }: { icon: ReactNode; title: string; description: string }) {
  return (
    <div className="panel-title">
      <span className="panel-icon">{icon}</span>
      <div>
        <h2>{title}</h2>
        <p>{description}</p>
      </div>
    </div>
  );
}

function Metric({ label, value }: { label: ReactNode; value: ReactNode }) {
  return (
    <div className="metric">
      <strong>{value}</strong>
      <span>{label}</span>
    </div>
  );
}

function Alert({ type, children }: { type: AlertType; children: ReactNode }) {
  return <div className={`alert alert-${type || 'info'}`}>{children}</div>;
}

function StatusBadge({ status }: { status: TaskStatus | AccountStatus }) {
  return <span className={`status-badge status-${status}`}>{statusLabels[status] || status}</span>;
}

function BackgroundParticles() {
  return (
    <div className="particles" aria-hidden="true">
      {Array.from({ length: 14 }).map((_, index) => (
        <span key={index} style={{ '--i': index } as React.CSSProperties} />
      ))}
    </div>
  );
}

function routeFromPath(pathname: string): RouteName {
  return pathname === '/admin' ? 'admin' : 'workspace';
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

function formatTimestamp(value: string) {
  const numeric = Number(value);
  if (Number.isFinite(numeric) && numeric > 0) {
    return new Date(numeric * 1000).toLocaleString();
  }
  return value;
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
