use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{Arc, RwLock},
};

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use chrono::Local;
use serde::{Deserialize, Serialize};
use tokio::time::{Duration, sleep};
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use uuid::Uuid;

const DEMO_CARD_KEY: &str = "demo-card-key";

#[derive(Clone)]
struct AppState {
    cards: Arc<RwLock<HashMap<String, Card>>>,
    tasks: Arc<RwLock<HashMap<String, Task>>>,
    settings: Settings,
}

#[derive(Clone)]
struct Card {
    total_units: u32,
    remaining_units: u32,
}

#[derive(Clone, Serialize)]
struct Settings {
    auto_bind_enabled: bool,
    auto_bind_one_dollar_enabled: bool,
}

#[derive(Clone, Serialize)]
struct Task {
    task_id: String,
    #[serde(skip_serializing)]
    card_key: String,
    #[serde(skip_serializing)]
    service_type: ServiceType,
    #[serde(skip_serializing)]
    cost_units: u32,
    status: TaskStatus,
    total_accounts: usize,
    accounts: Vec<TaskAccount>,
    created_at: String,
}

#[derive(Clone, Serialize)]
struct TaskAccount {
    id: u64,
    line_number: usize,
    email: String,
    status: AccountStatus,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    result_link: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    queue_position: Option<usize>,
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum ServiceType {
    LinkOnly,
    LinkAndBind,
    #[serde(rename = "link_and_bind_1usd")]
    LinkAndBind1usd,
}

impl Default for ServiceType {
    fn default() -> Self {
        Self::LinkOnly
    }
}

impl ServiceType {
    fn cost_units(self) -> u32 {
        match self {
            Self::LinkOnly => 1,
            Self::LinkAndBind => 2,
            Self::LinkAndBind1usd => 3,
        }
    }

    fn success_status(self) -> AccountStatus {
        match self {
            Self::LinkOnly => AccountStatus::Success,
            Self::LinkAndBind | Self::LinkAndBind1usd => AccountStatus::BindSuccess,
        }
    }

    fn success_message(self) -> &'static str {
        match self {
            Self::LinkOnly => "处理成功",
            Self::LinkAndBind | Self::LinkAndBind1usd => "绑卡成功",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum TaskStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum AccountStatus {
    Pending,
    Running,
    Success,
    Failed,
    BindPending,
    Binding,
    BindSuccess,
    BindFailed,
    Cancelled,
}

impl AccountStatus {
    fn is_success(self) -> bool {
        matches!(self, Self::Success | Self::BindSuccess)
    }

    fn is_failed(self) -> bool {
        matches!(self, Self::Failed | Self::BindFailed)
    }

    fn is_done(self) -> bool {
        matches!(
            self,
            Self::Success | Self::Failed | Self::BindSuccess | Self::BindFailed | Self::Cancelled
        )
    }

    fn is_exportable(self) -> bool {
        matches!(
            self,
            Self::Success
                | Self::BindPending
                | Self::Binding
                | Self::BindSuccess
                | Self::BindFailed
        )
    }
}

#[derive(Deserialize)]
struct CardRequest {
    card_key: String,
}

#[derive(Deserialize)]
struct SubmitTaskRequest {
    card_key: String,
    accounts_text: String,
    #[serde(default)]
    service_type: ServiceType,
}

#[derive(Deserialize)]
struct TasksByCardRequest {
    card_key: String,
    #[serde(default)]
    account_query: String,
}

#[derive(Serialize)]
struct VerifyCardResponse {
    valid: bool,
    remaining: Option<u32>,
    total_count: Option<u32>,
    remaining_quota: Option<String>,
    total_quota: Option<String>,
    remaining_quota_units: Option<u32>,
    total_quota_units: Option<u32>,
    message: String,
}

#[derive(Serialize)]
struct SubmitTaskResponse {
    task_id: String,
    total_accounts: usize,
    message: String,
}

#[derive(Serialize)]
struct TasksByCardResponse {
    tasks: Vec<TaskSummary>,
    message: String,
}

#[derive(Serialize)]
struct TaskSummary {
    task_id: String,
    total_accounts: usize,
    status: TaskStatus,
    success: usize,
    failed: usize,
    cancelled: usize,
    done: usize,
    created_at: String,
}

#[derive(Serialize)]
struct ExportByCardResponse {
    accounts: Vec<ExportAccount>,
    message: String,
}

#[derive(Serialize)]
struct ExportAccount {
    email: String,
    result_link: String,
    task_id: String,
    line_number: usize,
}

#[derive(Serialize)]
struct MessageResponse {
    message: String,
}

#[derive(Serialize)]
struct ErrorResponse {
    detail: String,
}

struct ApiError {
    status: StatusCode,
    detail: String,
}

impl ApiError {
    fn bad_request(detail: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            detail: detail.into(),
        }
    }

    fn not_found(detail: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            detail: detail.into(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(ErrorResponse {
                detail: self.detail,
            }),
        )
            .into_response()
    }
}

type ApiResult<T> = Result<Json<T>, ApiError>;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            std::env::var("RUST_LOG")
                .unwrap_or_else(|_| "pixel_api=debug,tower_http=debug".to_string()),
        )
        .init();

    let state = AppState::demo();
    let app = Router::new()
        .route("/api/settings", get(settings))
        .route("/api/verify-card", post(verify_card))
        .route("/api/submit-task", post(submit_task))
        .route("/api/task/{task_id}", get(task_detail))
        .route("/api/tasks-by-card", post(tasks_by_card))
        .route("/api/tasks/export-by-card", post(export_by_card))
        .route(
            "/api/task/{task_id}/account/{account_id}/cancel-queue",
            post(cancel_queue),
        )
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let addr = SocketAddr::from(([127, 0, 0, 1], 8080));
    println!("pixel-api listening on http://{addr}");

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("bind API address");
    axum::serve(listener, app).await.expect("run API server");
}

impl AppState {
    fn demo() -> Self {
        let mut cards = HashMap::new();
        cards.insert(
            DEMO_CARD_KEY.to_string(),
            Card {
                total_units: 20,
                remaining_units: 20,
            },
        );

        Self {
            cards: Arc::new(RwLock::new(cards)),
            tasks: Arc::new(RwLock::new(HashMap::new())),
            settings: Settings {
                auto_bind_enabled: true,
                auto_bind_one_dollar_enabled: false,
            },
        }
    }
}

async fn settings(State(state): State<AppState>) -> Json<Settings> {
    Json(state.settings)
}

async fn verify_card(
    State(state): State<AppState>,
    Json(payload): Json<CardRequest>,
) -> Json<VerifyCardResponse> {
    Json(card_response(&state, payload.card_key.trim()))
}

async fn submit_task(
    State(state): State<AppState>,
    Json(payload): Json<SubmitTaskRequest>,
) -> ApiResult<SubmitTaskResponse> {
    let card_key = payload.card_key.trim().to_string();
    if card_key.is_empty() {
        return Err(ApiError::bad_request("请输入卡密"));
    }

    let accounts = parse_accounts(&payload.accounts_text);
    if accounts.is_empty() {
        return Err(ApiError::bad_request("请输入账号信息"));
    }

    let cost_units = payload.service_type.cost_units();
    let required_units = cost_units * accounts.len() as u32;
    {
        let mut cards = state.cards.write().expect("cards lock");
        let card = cards
            .get_mut(&card_key)
            .ok_or_else(|| ApiError::bad_request("卡密不存在"))?;
        if card.remaining_units < required_units {
            return Err(ApiError::bad_request("额度不足"));
        }
        card.remaining_units -= required_units;
    }

    let task_id = Uuid::new_v4().to_string();
    let total_accounts = accounts.len();
    let task_accounts = accounts
        .into_iter()
        .enumerate()
        .map(|(index, (line_number, email))| TaskAccount {
            id: (index + 1) as u64,
            line_number,
            email,
            status: AccountStatus::Pending,
            message: "排队中".to_string(),
            result_link: None,
            queue_position: Some(index + 1),
        })
        .collect::<Vec<_>>();

    let task = Task {
        task_id: task_id.clone(),
        card_key,
        service_type: payload.service_type,
        cost_units,
        status: TaskStatus::Running,
        total_accounts,
        accounts: task_accounts,
        created_at: Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
    };

    state
        .tasks
        .write()
        .expect("tasks lock")
        .insert(task_id.clone(), task);

    tokio::spawn(process_task(state.clone(), task_id.clone()));

    Ok(Json(SubmitTaskResponse {
        task_id,
        total_accounts,
        message: "任务提交成功".to_string(),
    }))
}

async fn task_detail(
    State(state): State<AppState>,
    Path(task_id): Path<String>,
) -> ApiResult<Task> {
    let task = state
        .tasks
        .read()
        .expect("tasks lock")
        .get(&task_id)
        .cloned()
        .ok_or_else(|| ApiError::not_found("任务不存在"))?;

    Ok(Json(task))
}

async fn tasks_by_card(
    State(state): State<AppState>,
    Json(payload): Json<TasksByCardRequest>,
) -> ApiResult<TasksByCardResponse> {
    ensure_card_exists(&state, &payload.card_key)?;
    let query = payload.account_query.trim().to_lowercase();
    let mut tasks = state
        .tasks
        .read()
        .expect("tasks lock")
        .values()
        .filter(|task| task.card_key == payload.card_key.trim())
        .filter(|task| task_matches_query(task, &query))
        .map(task_summary)
        .collect::<Vec<_>>();

    tasks.sort_by(|a, b| b.created_at.cmp(&a.created_at));

    Ok(Json(TasksByCardResponse {
        tasks,
        message: "查询成功".to_string(),
    }))
}

async fn export_by_card(
    State(state): State<AppState>,
    Json(payload): Json<TasksByCardRequest>,
) -> ApiResult<ExportByCardResponse> {
    ensure_card_exists(&state, &payload.card_key)?;
    let query = payload.account_query.trim().to_lowercase();
    let tasks = state.tasks.read().expect("tasks lock");
    let mut accounts = Vec::new();

    for task in tasks
        .values()
        .filter(|task| task.card_key == payload.card_key.trim())
    {
        if !task_matches_query(task, &query) {
            continue;
        }
        for account in &task.accounts {
            if account.status.is_exportable() {
                if let Some(result_link) = &account.result_link {
                    accounts.push(ExportAccount {
                        email: account.email.clone(),
                        result_link: result_link.clone(),
                        task_id: task.task_id.clone(),
                        line_number: account.line_number,
                    });
                }
            }
        }
    }

    Ok(Json(ExportByCardResponse {
        accounts,
        message: "查询成功".to_string(),
    }))
}

async fn cancel_queue(
    State(state): State<AppState>,
    Path((task_id, account_id)): Path<(String, u64)>,
) -> ApiResult<MessageResponse> {
    let refund: (String, u32);
    {
        let mut tasks = state.tasks.write().expect("tasks lock");
        let task = tasks
            .get_mut(&task_id)
            .ok_or_else(|| ApiError::not_found("任务不存在"))?;
        let account = task
            .accounts
            .iter_mut()
            .find(|account| account.id == account_id)
            .ok_or_else(|| ApiError::not_found("账号不存在"))?;

        if account.status != AccountStatus::Pending {
            return Err(ApiError::bad_request("账号不在排队中"));
        }

        account.status = AccountStatus::Cancelled;
        account.message = "已取消".to_string();
        account.queue_position = None;
        refund = (task.card_key.clone(), task.cost_units);

        update_task_status(task);
    }

    if let Some(card) = state.cards.write().expect("cards lock").get_mut(&refund.0) {
        card.remaining_units = (card.remaining_units + refund.1).min(card.total_units);
    }

    Ok(Json(MessageResponse {
        message: "已取消".to_string(),
    }))
}

async fn process_task(state: AppState, task_id: String) {
    sleep(Duration::from_secs(1)).await;
    {
        let mut tasks = state.tasks.write().expect("tasks lock");
        if let Some(task) = tasks.get_mut(&task_id) {
            for account in &mut task.accounts {
                if account.status == AccountStatus::Pending {
                    account.status = match task.service_type {
                        ServiceType::LinkOnly => AccountStatus::Running,
                        ServiceType::LinkAndBind | ServiceType::LinkAndBind1usd => {
                            AccountStatus::BindPending
                        }
                    };
                    account.message = match account.status {
                        AccountStatus::BindPending => "待绑卡".to_string(),
                        _ => "运行中".to_string(),
                    };
                    account.queue_position = None;
                }
            }
        }
    }

    sleep(Duration::from_secs(1)).await;
    {
        let mut tasks = state.tasks.write().expect("tasks lock");
        if let Some(task) = tasks.get_mut(&task_id) {
            for account in &mut task.accounts {
                if account.status == AccountStatus::BindPending {
                    account.status = AccountStatus::Binding;
                    account.message = "绑卡中".to_string();
                }
            }
        }
    }

    sleep(Duration::from_secs(2)).await;
    let mut refunds: HashMap<String, u32> = HashMap::new();
    {
        let mut tasks = state.tasks.write().expect("tasks lock");
        if let Some(task) = tasks.get_mut(&task_id) {
            for account in &mut task.accounts {
                if account.status == AccountStatus::Cancelled {
                    continue;
                }

                if account.email.to_lowercase().contains("fail") {
                    account.status = match task.service_type {
                        ServiceType::LinkOnly => AccountStatus::Failed,
                        ServiceType::LinkAndBind | ServiceType::LinkAndBind1usd => {
                            AccountStatus::BindFailed
                        }
                    };
                    account.message = "处理失败".to_string();
                    *refunds.entry(task.card_key.clone()).or_insert(0) += task.cost_units;
                } else {
                    account.status = task.service_type.success_status();
                    account.message = task.service_type.success_message().to_string();
                    account.result_link = Some(format!(
                        "https://example.com/link/{}/{}",
                        &task.task_id[..8],
                        account.id
                    ));
                }
            }
            update_task_status(task);
        }
    }

    if !refunds.is_empty() {
        let mut cards = state.cards.write().expect("cards lock");
        for (card_key, units) in refunds {
            if let Some(card) = cards.get_mut(&card_key) {
                card.remaining_units = (card.remaining_units + units).min(card.total_units);
            }
        }
    }
}

fn parse_accounts(accounts_text: &str) -> Vec<(usize, String)> {
    accounts_text
        .lines()
        .enumerate()
        .filter_map(|(index, line)| {
            let line = line.trim();
            if line.is_empty() {
                return None;
            }
            let email = line.split("----").next().unwrap_or("").trim();
            Some((index + 1, email.to_string()))
        })
        .collect()
}

fn ensure_card_exists(state: &AppState, card_key: &str) -> Result<(), ApiError> {
    let cards = state.cards.read().expect("cards lock");
    if cards.contains_key(card_key.trim()) {
        Ok(())
    } else {
        Err(ApiError::bad_request("卡密不存在"))
    }
}

fn card_response(state: &AppState, card_key: &str) -> VerifyCardResponse {
    let cards = state.cards.read().expect("cards lock");
    if let Some(card) = cards.get(card_key) {
        VerifyCardResponse {
            valid: true,
            remaining: Some(card.remaining_units),
            total_count: Some(card.total_units),
            remaining_quota: Some(format_quota(card.remaining_units)),
            total_quota: Some(format_quota(card.total_units)),
            remaining_quota_units: Some(card.remaining_units),
            total_quota_units: Some(card.total_units),
            message: "卡密有效".to_string(),
        }
    } else {
        VerifyCardResponse {
            valid: false,
            remaining: None,
            total_count: None,
            remaining_quota: None,
            total_quota: None,
            remaining_quota_units: None,
            total_quota_units: None,
            message: "卡密不存在".to_string(),
        }
    }
}

fn format_quota(units: u32) -> String {
    let whole = units / 2;
    if units % 2 == 0 {
        whole.to_string()
    } else {
        format!("{whole}.5")
    }
}

fn task_matches_query(task: &Task, query: &str) -> bool {
    if query.is_empty() {
        return true;
    }

    task.accounts
        .iter()
        .any(|account| account.email.to_lowercase().contains(query))
}

fn task_summary(task: &Task) -> TaskSummary {
    TaskSummary {
        task_id: task.task_id.clone(),
        total_accounts: task.total_accounts,
        status: task.status,
        success: task
            .accounts
            .iter()
            .filter(|account| account.status.is_success())
            .count(),
        failed: task
            .accounts
            .iter()
            .filter(|account| account.status.is_failed())
            .count(),
        cancelled: task
            .accounts
            .iter()
            .filter(|account| account.status == AccountStatus::Cancelled)
            .count(),
        done: task
            .accounts
            .iter()
            .filter(|account| account.status.is_done())
            .count(),
        created_at: task.created_at.clone(),
    }
}

fn update_task_status(task: &mut Task) {
    let done = task
        .accounts
        .iter()
        .filter(|account| account.status.is_done())
        .count();
    let cancelled = task
        .accounts
        .iter()
        .filter(|account| account.status == AccountStatus::Cancelled)
        .count();
    let failed = task
        .accounts
        .iter()
        .filter(|account| account.status.is_failed())
        .count();

    task.status = if cancelled == task.total_accounts {
        TaskStatus::Cancelled
    } else if done == task.total_accounts && failed == task.total_accounts {
        TaskStatus::Failed
    } else if done == task.total_accounts {
        TaskStatus::Completed
    } else if done > 0 {
        TaskStatus::Running
    } else {
        TaskStatus::Pending
    };
}
