import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import "./style.css";

type EmailServiceRecord = {
  id: number;
  service_type: string;
  name: string;
  base_url: string;
  has_api_key: boolean;
  prefix: string | null;
  enabled: boolean;
  priority: number;
  last_used: string | null;
};

type AccountRecord = {
  id: number;
  email: string;
  status: string;
  password?: string | null;
  workspace_id?: string | null;
  created_at: string;
};

type PreviewEmail = {
  email: string;
  service_id: string;
  created_at: string;
  inbox_token?: string | null;
};

type TaskSnapshot = {
  id: string;
  kind: "single" | "batch";
  status: "pending" | "running" | "completed" | "failed" | "cancelled";
  title: string;
  progress_total: number;
  progress_completed: number;
  success_count: number;
  failed_count: number;
  current_email?: string | null;
  logs: string[];
  error_message?: string | null;
  updated_at: string;
};

type BootstrapPayload = {
  services: EmailServiceRecord[];
  accounts: AccountRecord[];
  settings: AppSettings;
  accountStats: AccountStats;
  databaseInfo: DatabaseInfo;
};

type AppTab = "register" | "settings" | "mail" | "accounts";
type ServiceType = "gptmail" | "custom_domain";

type AppSettings = {
  proxyEnabled: boolean;
  proxyHttp: string;
  proxyHttps: string;
  proxyAll: string;
  openaiClientId: string;
  openaiAuthUrl: string;
  openaiTokenUrl: string;
  openaiRedirectUri: string;
  openaiScope: string;
  registrationTimeout: number;
  registrationMaxRetries: number;
  batchIntervalSeconds: number;
  cpaEnabled: boolean;
  cpaApiUrl: string;
  cpaApiToken: string;
};

type AccountStats = {
  total: number;
  active: number;
  failed: number;
  other: number;
};

type DatabaseInfo = {
  dbPath: string;
  fileSizeBytes: number;
  accountsCount: number;
  servicesCount: number;
  tasksCount: number;
};

type ExportFormat = "json" | "csv";

const state = {
  services: [] as EmailServiceRecord[],
  accounts: [] as AccountRecord[],
  previewEmails: [] as PreviewEmail[],
  tasks: [] as TaskSnapshot[],
  selectedTaskId: null as string | null,
  selectedRunServiceId: null as number | null,
  mode: "single" as "single" | "batch",
  activeTab: "register" as AppTab,
  selectedServiceType: "gptmail" as ServiceType,
  settings: {
    proxyEnabled: false,
    proxyHttp: "",
    proxyHttps: "",
    proxyAll: "",
    openaiClientId: "app_EMoamEEZ73f0CkXaXp7hrann",
    openaiAuthUrl: "https://auth.openai.com/oauth/authorize",
    openaiTokenUrl: "https://auth.openai.com/oauth/token",
    openaiRedirectUri: "http://localhost:1455/auth/callback",
    openaiScope: "openid email profile offline_access",
    registrationTimeout: 120,
    registrationMaxRetries: 3,
    batchIntervalSeconds: 3,
    cpaEnabled: false,
    cpaApiUrl: "",
    cpaApiToken: "",
  } as AppSettings,
  accountStats: { total: 0, active: 0, failed: 0, other: 0 } as AccountStats,
  databaseInfo: {
    dbPath: "",
    fileSizeBytes: 0,
    accountsCount: 0,
    servicesCount: 0,
    tasksCount: 0,
  } as DatabaseInfo,
  selectedAccountIds: [] as number[],
  logFeed: [] as string[],
};

let taskPollingHandle: number | undefined;
let taskUnlisten: UnlistenFn | undefined;
const isTauriRuntime =
  typeof window !== "undefined" &&
  (typeof ((window as unknown) as Record<string, unknown>).__TAURI_INTERNALS__ !== "undefined" ||
    typeof ((window as unknown) as Record<string, unknown>).__TAURI__ !== "undefined");

function currentService(): EmailServiceRecord | undefined {
  return state.services.find((service) => service.enabled) ?? state.services[0];
}

function currentRunnableService(): EmailServiceRecord | undefined {
  return currentService();
}

function serviceByType(type: ServiceType): EmailServiceRecord | undefined {
  return state.services.find((service) => service.service_type === type) ?? currentService();
}

function selectedTask(): TaskSnapshot | undefined {
  return state.tasks.find((task) => task.id === state.selectedTaskId) ?? state.tasks[0];
}

function pushSystemLog(message: string) {
  const stamp = new Date().toLocaleTimeString("zh-CN", { hour12: false });
  state.logFeed.unshift(`[${stamp}] ${message}`);
  state.logFeed = state.logFeed.slice(0, 140);
}

function render() {
  const app = document.querySelector<HTMLDivElement>("#app");
  if (!app) return;

  app.innerHTML = `
    <div class="app-shell">
      <div class="surface-glow glow-left"></div>
      <div class="surface-glow glow-right"></div>

      <aside class="sidebar">
        <div class="brand-panel">
          <span class="brand-tag">GPTGO</span>
          <h1>快速注册工具</h1>
          <p>一个快速、方便的桌面工具，用来管理注册、邮件、账号和日志。</p>
          <div class="runtime-badge ${isTauriRuntime ? "live" : "preview"}">
            ${isTauriRuntime ? "Tauri Runtime" : "Browser Preview"}
          </div>
        </div>

        <nav class="nav-stack" id="tabbar">
          ${renderTabs()}
        </nav>

        <section class="sidebar-card">
          <div class="sidebar-card-head">
            <strong>当前服务</strong>
            <button id="reload-btn" class="ghost-button compact">刷新</button>
          </div>
          ${
            currentRunnableService()
              ? `
                <div class="sidebar-meta">
                  <span>名称</span>
                  <strong>${escapeHtml(currentRunnableService()!.name)}</strong>
                </div>
                <div class="sidebar-meta">
                  <span>类型</span>
                  <strong>${escapeHtml(currentRunnableService()!.service_type)}</strong>
                </div>
                <div class="sidebar-meta">
                  <span>上次使用</span>
                  <strong>${currentRunnableService()!.last_used ? new Date(currentRunnableService()!.last_used!).toLocaleString("zh-CN") : "暂无"}</strong>
                </div>
              `
              : `<div class="empty-panel compact">先去“设置”页保存 GPTMail 服务。</div>`
          }
        </section>
      </aside>

      <section class="workspace-shell">
        ${renderActiveTab()}
      </section>

      <aside class="activity-shell">
        <div class="activity-head">
          <div>
            <span class="section-kicker">Activity</span>
            <h2>实时日志</h2>
          </div>
          <button id="refresh-tasks-btn" class="ghost-button compact">刷新任务</button>
        </div>
        <div class="activity-summary">
          <div class="activity-item">
            <span>当前任务</span>
            <strong>${selectedTask() ? escapeHtml(selectedTask()!.title) : "系统"}</strong>
          </div>
          <div class="activity-item">
            <span>状态</span>
            <strong>${selectedTask() ? statusText(selectedTask()!.status) : "空闲"}</strong>
          </div>
        </div>
        <div class="console-board">
          ${renderTaskLog()}
        </div>
      </aside>
    </div>
  `;

  bindEvents();
}

function renderStats() {
  const running = state.tasks.filter((task) => task.status === "running").length;
  const completed = state.tasks.filter((task) => task.status === "completed").length;
  const available = state.services.filter((service) => service.enabled).length;

  return `
    <article class="stat-card ember">
      <span>服务</span>
      <strong>${available}</strong>
      <p>可用邮箱源</p>
    </article>
    <article class="stat-card gold">
      <span>邮箱</span>
      <strong>${state.previewEmails.length}</strong>
      <p>待确认</p>
    </article>
    <article class="stat-card pine">
      <span>任务</span>
      <strong>${running}</strong>
      <p>运行中</p>
    </article>
    <article class="stat-card night">
      <span>账号</span>
      <strong>${state.accounts.length}</strong>
      <p>本地记录</p>
    </article>
    <article class="stat-card smoke">
      <span>完成</span>
      <strong>${completed}</strong>
      <p>任务总数</p>
    </article>
  `;
}

function renderTabs() {
  const tabs: Array<{ id: AppTab; label: string; note: string }> = [
    { id: "register", label: "注册", note: "执行单次 / 批量任务" },
    { id: "accounts", label: "账号", note: "本地账号结果" },
    { id: "mail", label: "邮件", note: "预生成邮箱与确认流" },
    { id: "settings", label: "设置", note: "邮箱服务与运行参数" },
  ];

  return tabs
    .map(
      (tab) => `
        <button type="button" class="tab-chip ${state.activeTab === tab.id ? "active" : ""}" data-tab="${tab.id}">
          <strong>${tab.label}</strong>
          <span>${tab.note}</span>
        </button>
      `,
    )
    .join("");
}

function renderActiveTab() {
  switch (state.activeTab) {
    case "settings":
      return renderSettingsTab();
    case "mail":
      return renderMailTab();
    case "accounts":
      return renderAccountsTab();
    case "register":
    default:
      return renderRegisterTab();
  }
}

function renderRegisterTab() {
  const runnableServices = state.services.filter((service) => service.enabled);
  const selectedRunService =
    runnableServices.find((service) => service.id === state.selectedRunServiceId) ?? runnableServices[0];

  return `
    <section class="content-grid register-grid">
      <article class="panel register-workbench wide-panel">
        <div class="panel-head">
          <div>
            <p class="section-kicker">Overview</p>
            <h2>注册概览</h2>
          </div>
        </div>
        <div class="top-metrics">
          ${renderStats()}
        </div>

        <div class="register-main-grid">
          <section class="workbench-section">
            <div class="panel-head compact-head">
              <div>
                <p class="section-kicker">Registration</p>
                <h2>注册调度</h2>
              </div>
              <div class="mode-pills" id="mode-switch">
                <button type="button" class="mini-pill ${state.mode === "single" ? "active" : ""}" data-mode="single">单次</button>
                <button type="button" class="mini-pill ${state.mode === "batch" ? "active" : ""}" data-mode="batch">批量</button>
              </div>
            </div>

            <form id="runner-form" class="form-stack">
              <div class="form-group-inline">
                <span class="field-label">邮箱服务</span>
                <div class="service-choice-grid">
                  ${
                    runnableServices.length > 0
                      ? runnableServices
                          .map(
                            (service) => `
                              <button
                                type="button"
                                class="service-choice ${selectedRunService?.id === service.id ? "active" : ""}"
                                data-run-service-id="${service.id}"
                              >
                                <strong>${escapeHtml(service.name)}</strong>
                                <span>${escapeHtml(service.service_type)}</span>
                              </button>
                            `,
                          )
                          .join("")
                      : `<div class="empty-panel compact">请先在“设置”页保存并启用邮件服务。</div>`
                  }
                </div>
              </div>
              <div class="split-fields">
                <label>
                  <span>${state.mode === "single" ? "预生成数量" : "批量数量"}</span>
                  <input name="count" type="number" min="1" max="20" value="${state.mode === "single" ? 1 : Math.max(state.previewEmails.length, 3)}" />
                </label>
                <label>
                  <span>批量间隔 (秒)</span>
                  <input name="interval" type="number" min="0" max="60" value="3" ${state.mode === "single" ? "disabled" : ""} />
                </label>
              </div>
              <div class="button-row">
                <button type="submit" class="primary-button">预生成邮箱</button>
                <button type="button" id="confirm-preview-btn" class="dark-button">确认并执行注册</button>
              </div>
            </form>

            <div class="hint-box compact-note">
              <strong>当前选择</strong>
              <p>${
                selectedRunService
                  ? `使用 ${escapeHtml(selectedRunService.name)}，先生成邮箱，确认后才启动注册。`
                  : "先在设置页保存一个可用的邮件服务。"
              }</p>
            </div>
          </section>

          <section class="workbench-section">
            <div class="panel-head compact-head">
              <div>
                <p class="section-kicker">Tasks</p>
                <h2>任务列表</h2>
              </div>
            </div>
            <div class="task-list">
              ${renderTaskCards()}
            </div>
          </section>
        </div>
      </article>
    </section>
  `;
}

function renderSettingsTab() {
  const serviceType = state.selectedServiceType;
  const service = serviceByType(serviceType);
  const isCustomDomain = serviceType === "custom_domain";
  return `
    <section class="content-grid settings-grid">
      <article class="panel">
        <div class="panel-head">
          <div>
            <p class="section-kicker">Mail Channels</p>
            <h2>${isCustomDomain ? "自定义邮箱 API 设置" : "GPTMail 服务设置"}</h2>
          </div>
        </div>
        <form id="service-form" class="form-stack">
          <div class="form-group-inline">
            <span class="field-label">通道类型</span>
            <div class="channel-switch" id="service-type-switch">
              <button type="button" class="channel-chip ${serviceType === "gptmail" ? "active" : ""}" data-service-type="gptmail">
                GPTMail
              </button>
              <button type="button" class="channel-chip ${serviceType === "custom_domain" ? "active" : ""}" data-service-type="custom_domain">
                自定义邮箱 API
              </button>
            </div>
          </div>
          <label>
            <span>服务名称</span>
            <input
              name="name"
              value="${escapeAttr(service?.name ?? (isCustomDomain ? "自定义邮箱主服务" : "GPTMail 主服务"))}"
              placeholder="${isCustomDomain ? "自定义邮箱主服务" : "GPTMail 主服务"}"
              required
            />
          </label>
          <label>
            <span>API 地址</span>
            <input
              name="baseUrl"
              value="${escapeAttr(service?.base_url ?? (isCustomDomain ? "https://api.example.com" : "https://mail.chatgpt.org.uk/api"))}"
              placeholder="${isCustomDomain ? "https://api.example.com" : "https://mail.chatgpt.org.uk/api"}"
              required
            />
          </label>
          <label>
            <span>API Key</span>
            <input name="apiKey" placeholder="${service?.has_api_key ? "已配置，重新填写可覆盖" : "sk-..."}" required />
          </label>
          <label>
            <span>${isCustomDomain ? "默认域名 / 前缀" : "默认前缀"}</span>
            <input
              name="prefix"
              value="${escapeAttr(service?.prefix ?? "")}"
              placeholder="${isCustomDomain ? "example.com" : "可留空"}"
            />
          </label>
          <div class="button-row">
            <button type="submit" class="primary-button">保存服务</button>
            <button type="button" id="test-service-btn" class="secondary-button">测试连通性</button>
          </div>
        </form>
        <div class="hint-box soft">
          <strong>提示</strong>
          <p>${isCustomDomain ? "自定义邮箱 API 已接入预生成邮箱、收验证码和注册执行链路。" : "GPTMail 已接入完整预生成、收验证码和注册执行链路。"}</p>
        </div>
      </article>

      <article class="panel">
        <div class="panel-head">
          <div>
            <p class="section-kicker">Proxy</p>
            <h2>代理设置</h2>
          </div>
        </div>
        <form id="proxy-form" class="form-stack">
          <label class="toggle-row">
            <span>启用代理</span>
            <input name="proxyEnabled" type="checkbox" ${state.settings.proxyEnabled ? "checked" : ""} />
          </label>
          <label>
            <span>HTTP Proxy</span>
            <input name="proxyHttp" value="${escapeAttr(state.settings.proxyHttp)}" placeholder="http://127.0.0.1:7897" />
          </label>
          <label>
            <span>HTTPS Proxy</span>
            <input name="proxyHttps" value="${escapeAttr(state.settings.proxyHttps)}" placeholder="http://127.0.0.1:7897" />
          </label>
          <label>
            <span>ALL Proxy</span>
            <input name="proxyAll" value="${escapeAttr(state.settings.proxyAll)}" placeholder="socks5://127.0.0.1:7897" />
          </label>
          <div class="button-row">
            <button type="submit" class="primary-button">保存代理</button>
          </div>
        </form>
      </article>

      <article class="panel wide-panel">
        <div class="panel-head">
          <div>
            <p class="section-kicker">Global</p>
            <h2>全局设置</h2>
          </div>
        </div>
        <form id="global-settings-form" class="form-stack">
          <div class="split-fields">
            <label>
              <span>OpenAI Client ID</span>
              <input name="openaiClientId" value="${escapeAttr(state.settings.openaiClientId)}" />
            </label>
            <label>
              <span>OAuth Scope</span>
              <input name="openaiScope" value="${escapeAttr(state.settings.openaiScope)}" />
            </label>
          </div>
          <div class="split-fields">
            <label>
              <span>Auth URL</span>
              <input name="openaiAuthUrl" value="${escapeAttr(state.settings.openaiAuthUrl)}" />
            </label>
            <label>
              <span>Token URL</span>
              <input name="openaiTokenUrl" value="${escapeAttr(state.settings.openaiTokenUrl)}" />
            </label>
          </div>
          <div class="split-fields">
            <label>
              <span>Redirect URI</span>
              <input name="openaiRedirectUri" value="${escapeAttr(state.settings.openaiRedirectUri)}" />
            </label>
            <label>
              <span>注册超时 (秒)</span>
              <input name="registrationTimeout" type="number" min="30" max="600" value="${state.settings.registrationTimeout}" />
            </label>
          </div>
          <div class="split-fields">
            <label>
              <span>最大重试</span>
              <input name="registrationMaxRetries" type="number" min="1" max="10" value="${state.settings.registrationMaxRetries}" />
            </label>
            <label>
              <span>默认批量间隔 (秒)</span>
              <input name="batchIntervalSeconds" type="number" min="0" max="60" value="${state.settings.batchIntervalSeconds}" />
            </label>
          </div>
          <div class="button-row">
            <button type="submit" class="primary-button">保存全局设置</button>
          </div>
        </form>
      </article>

      <article class="panel wide-panel">
        <div class="panel-head">
          <div>
            <p class="section-kicker">CPA</p>
            <h2>CPA 设置</h2>
          </div>
        </div>
        <form id="cpa-form" class="form-stack">
          <label class="toggle-row">
            <span>启用 CPA</span>
            <input name="cpaEnabled" type="checkbox" ${state.settings.cpaEnabled ? "checked" : ""} />
          </label>
          <label>
            <span>CPA API URL</span>
            <input name="cpaApiUrl" value="${escapeAttr(state.settings.cpaApiUrl)}" placeholder="https://your-cpa-api.example.com" />
          </label>
          <label>
            <span>CPA Token</span>
            <input name="cpaApiToken" value="${escapeAttr(state.settings.cpaApiToken)}" placeholder="Bearer Token" />
          </label>
          <div class="button-row">
            <button type="submit" class="primary-button">保存 CPA 设置</button>
          </div>
        </form>
      </article>

      <article class="panel wide-panel">
        <div class="panel-head">
          <div>
            <p class="section-kicker">Saved Services</p>
            <h2>已保存服务</h2>
          </div>
        </div>
        <div class="service-card-list">
          ${renderServiceList()}
        </div>
      </article>

      <article class="panel wide-panel">
        <div class="panel-head">
          <div>
            <p class="section-kicker">Database</p>
            <h2>数据库管理</h2>
          </div>
        </div>
        <div class="db-grid">
          <div class="db-card"><span>数据库路径</span><code>${escapeHtml(state.databaseInfo.dbPath || "未初始化")}</code></div>
          <div class="db-card"><span>文件大小</span><strong>${formatBytes(state.databaseInfo.fileSizeBytes)}</strong></div>
          <div class="db-card"><span>账号数</span><strong>${state.databaseInfo.accountsCount}</strong></div>
          <div class="db-card"><span>服务数</span><strong>${state.databaseInfo.servicesCount}</strong></div>
          <div class="db-card"><span>任务数</span><strong>${state.databaseInfo.tasksCount}</strong></div>
        </div>
        <div class="button-row">
          <button type="button" id="backup-db-btn" class="secondary-button">备份数据库</button>
          <button type="button" id="clear-tasks-btn" class="ghost-button">清理任务记录</button>
        </div>
      </article>
    </section>
  `;
}

function renderMailTab() {
  return `
    <section class="content-grid mail-grid">
      <article class="panel wide-panel">
        <div class="panel-head">
          <div>
            <p class="section-kicker">Mail</p>
            <h2>预生成邮箱池</h2>
          </div>
          <span class="status-badge system">${state.previewEmails.length} 个待确认</span>
        </div>
        <div class="table-shell">
          <table>
            <thead>
              <tr>
                <th>#</th>
                <th>邮箱地址</th>
                <th>生成时间</th>
              </tr>
            </thead>
            <tbody>${renderPreviewRows()}</tbody>
          </table>
        </div>
      </article>

      <article class="panel">
        <div class="panel-head">
          <div>
            <p class="section-kicker">Workflow</p>
            <h2>执行说明</h2>
          </div>
        </div>
        <ol class="flow-list">
          <li>在“注册”页选择单次或批量，先点“预生成邮箱”。</li>
          <li>邮箱会先出现在这里，不会立即触发注册。</li>
          <li>你确认后，Rust 后端才会真正开始跑注册任务。</li>
          <li>任务执行过程和验证码拉取情况会回流到任务控制台。</li>
        </ol>
      </article>

      <article class="panel">
        <div class="panel-head">
          <div>
            <p class="section-kicker">Health</p>
            <h2>邮件服务摘要</h2>
          </div>
        </div>
        <div class="meta-list">
          <div class="meta-row"><span>已配置服务数</span><strong>${state.services.length}</strong></div>
          <div class="meta-row"><span>待确认邮箱数</span><strong>${state.previewEmails.length}</strong></div>
          <div class="meta-row"><span>最近任务邮箱</span><strong>${selectedTask()?.current_email ? escapeHtml(selectedTask()!.current_email!) : "暂无"}</strong></div>
        </div>
      </article>
    </section>
  `;
}

function renderAccountsTab() {
  return `
    <section class="content-grid accounts-grid">
      <article class="panel wide-panel">
        <div class="panel-head">
          <div>
            <p class="section-kicker">Stats</p>
            <h2>账号统计</h2>
          </div>
        </div>
        <div class="top-metrics compact-metrics">
          <article class="stat-card ember"><span>总数</span><strong>${state.accountStats.total}</strong><p>全部账号</p></article>
          <article class="stat-card pine"><span>活跃</span><strong>${state.accountStats.active}</strong><p>可用账号</p></article>
          <article class="stat-card night"><span>失败</span><strong>${state.accountStats.failed}</strong><p>失败账号</p></article>
          <article class="stat-card smoke"><span>其他</span><strong>${state.accountStats.other}</strong><p>其他状态</p></article>
        </div>
      </article>

      <article class="panel wide-panel">
        <div class="panel-head">
          <div>
            <p class="section-kicker">Accounts</p>
            <h2>账号记录</h2>
          </div>
          <span class="status-badge system">${state.accounts.length} 条记录</span>
        </div>
        <div class="button-row account-toolbar">
          <button type="button" id="refresh-accounts-btn" class="ghost-button">刷新账号</button>
          <button type="button" id="refresh-token-btn" class="secondary-button">刷新 Token</button>
          <button type="button" id="validate-token-btn" class="secondary-button">验证 Token</button>
          <button type="button" id="batch-validate-btn" class="ghost-button">批量验证</button>
          <button type="button" id="mark-active-btn" class="primary-button">标记活跃</button>
          <button type="button" id="mark-failed-btn" class="ghost-button">标记失败</button>
          <button type="button" id="export-json-btn" class="secondary-button">导出 JSON</button>
          <button type="button" id="export-csv-btn" class="secondary-button">导出 CSV</button>
          <button type="button" id="export-cpa-btn" class="secondary-button">导出 CPA</button>
          <button type="button" id="upload-cpa-btn" class="primary-button">上传 CPA</button>
          <button type="button" id="delete-accounts-btn" class="ghost-button danger">删除选中</button>
        </div>
        <div class="account-list">
          ${renderAccounts()}
        </div>
      </article>
    </section>
  `;
}

function renderPreviewRows() {
  if (state.previewEmails.length === 0) {
    return `<tr><td colspan="3" class="empty-cell">先去“注册”页预生成邮箱，这里会展示所有待确认邮箱。</td></tr>`;
  }

  return state.previewEmails
    .map(
      (item, index) => `
        <tr>
          <td>${index + 1}</td>
          <td>${escapeHtml(item.email)}</td>
          <td>${new Date(item.created_at).toLocaleTimeString("zh-CN", { hour12: false })}</td>
        </tr>
      `,
    )
    .join("");
}

function renderTaskCards() {
  if (state.tasks.length === 0) {
    return `<div class="empty-panel">任务列表还没有内容。确认执行注册后，这里才会开始出现任务卡片。</div>`;
  }

  return state.tasks
    .map((task) => {
      const selected = task.id === (state.selectedTaskId ?? selectedTask()?.id);
      const progress = Math.round((task.progress_completed / Math.max(task.progress_total, 1)) * 100);
      return `
        <button class="task-card ${task.status} ${selected ? "selected" : ""}" data-task-id="${task.id}">
          <div class="task-header">
            <span>${task.kind === "single" ? "单次任务" : "批量任务"}</span>
            <strong>${statusText(task.status)}</strong>
          </div>
          <h3>${escapeHtml(task.title)}</h3>
          <div class="task-meta">
            <span>${task.progress_completed}/${task.progress_total}</span>
            <span>✅ ${task.success_count}</span>
            <span>❌ ${task.failed_count}</span>
          </div>
          <div class="progress-track"><div class="progress-fill" style="width:${progress}%"></div></div>
          <p>${task.current_email ? escapeHtml(task.current_email) : "等待执行"}</p>
        </button>
      `;
    })
    .join("");
}

function renderTaskLog() {
  const task = selectedTask();
  if (!task) {
    return `<div class="console-line muted-log">当前没有任务。开始注册后，这里才会显示实时日志。</div>`;
  }

  const lines = task.logs.length > 0 ? task.logs : ["[系统] 任务已创建，等待输出"];
  return lines
    .slice()
    .reverse()
    .map((line) => `<div class="console-line">${escapeHtml(line)}</div>`)
    .join("");
}

function renderAccounts() {
  if (state.accounts.length === 0) {
    return `<div class="empty-panel">本地 SQLite 里还没有账号记录。</div>`;
  }

  return state.accounts
    .map(
      (account) => `
        <article class="account-card">
          <div class="account-main">
            <div>
              <label class="account-check">
                <input type="checkbox" data-account-id="${account.id}" ${state.selectedAccountIds.includes(account.id) ? "checked" : ""} />
                <strong>${escapeHtml(account.email)}</strong>
              </label>
              <p>${new Date(account.created_at).toLocaleString("zh-CN")}</p>
            </div>
            <span class="status-badge ${account.status}">${escapeHtml(account.status)}</span>
          </div>
          <div class="account-meta">
            <div><span>密码</span><code>${escapeHtml(account.password || "未记录")}</code></div>
            <div><span>Workspace</span><code>${escapeHtml(account.workspace_id || "未记录")}</code></div>
          </div>
        </article>
      `,
    )
    .join("");
}

function renderServiceList() {
  if (state.services.length === 0) {
    return `<div class="empty-panel">还没有已保存的邮箱服务。</div>`;
  }

  return state.services
    .map(
      (service) => `
        <article class="service-card">
          <div>
            <div class="service-title">
              <strong>${escapeHtml(service.name)}</strong>
              <span class="status-badge system">${escapeHtml(service.service_type)}</span>
            </div>
            <p>${escapeHtml(service.base_url)}</p>
          </div>
          <div class="service-foot">
            <span>${service.has_api_key ? "API Key 已配置" : "缺少 API Key"}</span>
            <span>${service.last_used ? new Date(service.last_used).toLocaleString("zh-CN") : "暂无使用记录"}</span>
          </div>
          <div class="button-row service-actions">
            <button type="button" class="ghost-button compact" data-service-test="${service.id}">测试</button>
            <button type="button" class="ghost-button compact" data-service-toggle="${service.id}" data-service-enabled="${service.enabled ? "1" : "0"}">${service.enabled ? "禁用" : "启用"}</button>
            <button type="button" class="ghost-button compact danger" data-service-delete="${service.id}">删除</button>
          </div>
        </article>
      `,
    )
    .join("");
}

async function backend<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (isTauriRuntime) {
    return invoke<T>(command, args);
  }
  return mockBackend<T>(command, args);
}

async function mockBackend<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  const services = readJson<EmailServiceRecord[]>("mock_services", []);
  const accounts = readJson<AccountRecord[]>("mock_accounts", []);
  const tasks = readJson<TaskSnapshot[]>("mock_tasks", []);
  const settings = readJson<AppSettings>("mock_settings", {
    proxyEnabled: false,
    proxyHttp: "",
    proxyHttps: "",
    proxyAll: "",
    openaiClientId: "app_EMoamEEZ73f0CkXaXp7hrann",
    openaiAuthUrl: "https://auth.openai.com/oauth/authorize",
    openaiTokenUrl: "https://auth.openai.com/oauth/token",
    openaiRedirectUri: "http://localhost:1455/auth/callback",
    openaiScope: "openid email profile offline_access",
    registrationTimeout: 120,
    registrationMaxRetries: 3,
    batchIntervalSeconds: 3,
    cpaEnabled: false,
    cpaApiUrl: "",
    cpaApiToken: "",
  });
  const accountStats = {
    total: accounts.length,
    active: accounts.filter((a) => a.status === "active").length,
    failed: accounts.filter((a) => a.status === "failed").length,
    other: accounts.filter((a) => a.status !== "active" && a.status !== "failed").length,
  };
  const databaseInfo = {
    dbPath: "Browser Preview",
    fileSizeBytes: 0,
    accountsCount: accounts.length,
    servicesCount: services.length,
    tasksCount: tasks.length,
  };

  switch (command) {
    case "bootstrap_app":
      return {
        services,
        accounts,
        settings,
        accountStats,
        databaseInfo,
      } as T;
    case "list_tasks":
      return tasks as T;
    case "save_email_service": {
      const input = (args?.input ?? {}) as Record<string, string>;
      const record: EmailServiceRecord = {
        id: services[0]?.id ?? 1,
        service_type: input.serviceType || "gptmail",
        name: input.name || "GPTMail 主服务",
        base_url: input.baseUrl || "https://mail.chatgpt.org.uk/api",
        has_api_key: Boolean(input.apiKey),
        prefix: input.prefix || null,
        enabled: true,
        priority: 0,
        last_used: new Date().toISOString(),
      };
      const others = services.filter((service) => service.service_type !== record.service_type);
      writeJson("mock_services", [record, ...others]);
      return record as T;
    }
    case "save_app_settings": {
      const input = (args?.input ?? {}) as AppSettings;
      writeJson("mock_settings", input);
      return input as T;
    }
    case "delete_accounts": {
      const ids = ((args?.input as { ids?: number[] } | undefined)?.ids ?? []) as number[];
      const next = accounts.filter((account) => !ids.includes(account.id));
      writeJson("mock_accounts", next);
      return { success: true, message: `已删除 ${ids.length} 个账号` } as T;
    }
    case "update_accounts_status": {
      const payload = (args?.input ?? {}) as { ids?: number[]; status?: string };
      const ids = payload.ids ?? [];
      const status = payload.status ?? "active";
      const next = accounts.map((account) => (ids.includes(account.id) ? { ...account, status } : account));
      writeJson("mock_accounts", next);
      return { success: true, message: `已更新 ${ids.length} 个账号状态` } as T;
    }
    case "export_accounts": {
      const payload = (args?.input ?? {}) as { ids?: number[]; format?: ExportFormat };
      const ids = payload.ids ?? [];
      const format = payload.format ?? "json";
      const selected = accounts.filter((account) => ids.includes(account.id));
      if (format === "csv") {
        const header = "id,email,status,password,workspace_id,created_at";
        const rows = selected.map((account) => [account.id, account.email, account.status, account.password ?? "", account.workspace_id ?? "", account.created_at].join(","));
        return { filename: "accounts-preview.csv", content: [header, ...rows].join("\n") } as T;
      }
      return { filename: "accounts-preview.json", content: JSON.stringify(selected, null, 2) } as T;
    }
    case "get_database_info":
      return databaseInfo as T;
    case "backup_database":
      return { success: true, backupPath: "Browser Preview" } as T;
    case "clear_registration_tasks":
      writeJson("mock_tasks", []);
      return { success: true, message: "注册任务记录已清空" } as T;
    case "delete_email_service": {
      const id = ((args?.input as { id?: number } | undefined)?.id ?? 0) as number;
      writeJson("mock_services", services.filter((service) => service.id !== id));
      return { success: true, message: "服务已删除" } as T;
    }
    case "toggle_email_service": {
      const id = (args?.id as number | undefined) ?? 0;
      const enabled = Boolean(args?.enabled);
      writeJson(
        "mock_services",
        services.map((service) => (service.id === id ? { ...service, enabled } : service)),
      );
      return { success: true, message: enabled ? "服务已启用" : "服务已禁用" } as T;
    }
    case "test_email_service":
      return { success: true, message: "浏览器预览模式: 这里展示界面，不执行真实服务测试。" } as T;
    case "preview_emails": {
      const request = (args?.request ?? {}) as Record<string, number>;
      const count = Number(request.count || 1);
      const now = new Date().toISOString();
      const emails = Array.from({ length: count }, (_, index) => ({
        email: `preview-${index + 1}-${Math.random().toString(16).slice(2, 8)}@example.test`,
        service_id: `preview-${index + 1}`,
        created_at: now,
        inbox_token: null,
      }));
      return emails as T;
    }
    case "start_single_registration": {
      const request = (args?.request ?? {}) as Record<string, PreviewEmail>;
      const email = request.previewEmail?.email ?? "preview@example.test";
      const task: TaskSnapshot = {
        id: crypto.randomUUID(),
        kind: "single",
        status: "completed",
        title: "单次注册",
        progress_total: 1,
        progress_completed: 1,
        success_count: 1,
        failed_count: 0,
        current_email: email,
        logs: [
          "[预览模式] 已创建模拟单次任务",
          `[预览模式] 使用邮箱: ${email}`,
          "[预览模式] 真正的 OpenAI 注册链路请在 Tauri 窗口中查看",
        ],
        error_message: null,
        updated_at: new Date().toISOString(),
      };
      writeJson("mock_tasks", [task, ...tasks]);
      return task as T;
    }
    case "start_batch_registration": {
      const request = (args?.request ?? {}) as Record<string, PreviewEmail[] | number>;
      const previewEmails = (request.previewEmails ?? []) as PreviewEmail[];
      const task: TaskSnapshot = {
        id: crypto.randomUUID(),
        kind: "batch",
        status: "completed",
        title: "批量注册",
        progress_total: Number(request.count || previewEmails.length || 1),
        progress_completed: Number(request.count || previewEmails.length || 1),
        success_count: previewEmails.length || Number(request.count || 1),
        failed_count: 0,
        current_email: previewEmails.length > 0 ? previewEmails[previewEmails.length - 1].email : null,
        logs: [
          "[预览模式] 已创建模拟批量任务",
          `[预览模式] 批量邮箱数: ${previewEmails.length || request.count || 1}`,
          "[预览模式] 用于展示 tab 结构与响应式布局",
        ],
        error_message: null,
        updated_at: new Date().toISOString(),
      };
      writeJson("mock_tasks", [task, ...tasks]);
      return task as T;
    }
    default:
      throw new Error(`浏览器预览模式暂不支持命令: ${command}`);
  }
}

function readJson<T>(key: string, fallback: T): T {
  try {
    const raw = window.localStorage.getItem(key);
    return raw ? (JSON.parse(raw) as T) : fallback;
  } catch {
    return fallback;
  }
}

function writeJson<T>(key: string, value: T) {
  window.localStorage.setItem(key, JSON.stringify(value));
}

function installGlobalUiActions() {
  (window as unknown as Record<string, unknown>).__gptgoSelectTab = (tab: AppTab) => {
    state.activeTab = tab;
    render();
  };

  (window as unknown as Record<string, unknown>).__gptgoSelectMode = (mode: "single" | "batch") => {
    state.mode = mode;
    if (mode === "single" && state.previewEmails.length > 1) {
      state.previewEmails = state.previewEmails.slice(0, 1);
    }
    render();
  };

  (window as unknown as Record<string, unknown>).__gptgoSelectServiceType = (serviceType: ServiceType) => {
    state.selectedServiceType = serviceType;
    render();
  };
}

function downloadTextFile(filename: string, content: string) {
  const blob = new Blob([content], { type: "text/plain;charset=utf-8" });
  const url = URL.createObjectURL(blob);
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = filename;
  anchor.click();
  URL.revokeObjectURL(url);
}

function selectedAccountIdsFromDom() {
  return Array.from(document.querySelectorAll<HTMLInputElement>('[data-account-id]:checked')).map((input) => Number(input.dataset.accountId));
}

function formatBytes(bytes: number) {
  if (!bytes) return "0 B";
  const units = ["B", "KB", "MB", "GB"];
  let value = bytes;
  let index = 0;
  while (value >= 1024 && index < units.length - 1) {
    value /= 1024;
    index += 1;
  }
  return `${value.toFixed(value >= 10 || index === 0 ? 0 : 1)} ${units[index]}`;
}

function bindEvents() {
  document.querySelector("#reload-btn")?.addEventListener("click", () => void bootstrap());
  document.querySelector("#refresh-tasks-btn")?.addEventListener("click", () => void refreshTasks());

  document.querySelectorAll<HTMLElement>("[data-tab]").forEach((element) => {
    element.addEventListener("click", () => {
      const tab = element.dataset.tab as AppTab | undefined;
      if (!tab) return;
      state.activeTab = tab;
      render();
    });
  });

  document.querySelectorAll<HTMLElement>("[data-mode]").forEach((element) => {
    element.addEventListener("click", () => {
      const mode = element.dataset.mode as ("single" | "batch" | undefined);
      if (!mode) return;
      state.mode = mode;
      if (mode === "single" && state.previewEmails.length > 1) {
        state.previewEmails = state.previewEmails.slice(0, 1);
      }
      render();
    });
  });

  document.querySelectorAll<HTMLElement>("[data-run-service-id]").forEach((element) => {
    element.addEventListener("click", () => {
      const id = Number(element.dataset.runServiceId);
      if (!Number.isFinite(id)) return;
      state.selectedRunServiceId = id;
      render();
    });
  });

  document.querySelectorAll<HTMLElement>("[data-service-type]").forEach((element) => {
    element.addEventListener("click", () => {
      const serviceType = element.dataset.serviceType as ServiceType | undefined;
      if (!serviceType) return;
      state.selectedServiceType = serviceType;
      render();
    });
  });

  document.querySelector<HTMLFormElement>("#service-form")?.addEventListener("submit", async (event) => {
    event.preventDefault();
    const form = event.currentTarget as HTMLFormElement | null;
    if (!form) return;
    const formData = new FormData(form);

    try {
      const service = await backend<EmailServiceRecord>("save_email_service", {
        input: {
          serviceType: state.selectedServiceType,
          name: String(formData.get("name") ?? ""),
          baseUrl: String(formData.get("baseUrl") ?? ""),
          apiKey: String(formData.get("apiKey") ?? ""),
          prefix: String(formData.get("prefix") ?? ""),
          enabled: true,
        },
      });
      state.selectedServiceType = service.service_type as ServiceType;
      pushSystemLog(`已保存邮箱服务: ${service.name}`);
      await bootstrap();
      state.activeTab = "register";
    } catch (error) {
      pushSystemLog(`保存服务失败: ${String(error)}`);
      render();
    }
  });

  document.querySelector("#test-service-btn")?.addEventListener("click", async () => {
    const service = serviceByType(state.selectedServiceType);
    if (!service) {
      pushSystemLog("请先保存邮箱服务");
      render();
      return;
    }

    try {
      const result = await backend<{ success: boolean; message: string }>("test_email_service", {
        serviceId: service.id,
      });
      pushSystemLog(result.message);
      await bootstrap();
    } catch (error) {
      pushSystemLog(`测试服务失败: ${String(error)}`);
      render();
    }
  });

  document.querySelectorAll<HTMLElement>("[data-service-test]").forEach((element) => {
    element.addEventListener("click", async () => {
      const serviceId = Number(element.dataset.serviceTest);
      try {
        const result = await backend<{ success: boolean; message: string }>("test_email_service", { serviceId });
        pushSystemLog(result.message);
        await bootstrap();
      } catch (error) {
        pushSystemLog(`测试服务失败: ${String(error)}`);
        render();
      }
    });
  });

  document.querySelectorAll<HTMLElement>("[data-service-toggle]").forEach((element) => {
    element.addEventListener("click", async () => {
      const id = Number(element.dataset.serviceToggle);
      const enabled = element.dataset.serviceEnabled !== "1";
      try {
        const result = await backend<{ message: string }>("toggle_email_service", { id, enabled });
        pushSystemLog(result.message);
        await bootstrap();
      } catch (error) {
        pushSystemLog(`切换服务失败: ${String(error)}`);
        render();
      }
    });
  });

  document.querySelectorAll<HTMLElement>("[data-service-delete]").forEach((element) => {
    element.addEventListener("click", async () => {
      const id = Number(element.dataset.serviceDelete);
      try {
        const result = await backend<{ message: string }>("delete_email_service", { input: { id } });
        pushSystemLog(result.message);
        await bootstrap();
      } catch (error) {
        pushSystemLog(`删除服务失败: ${String(error)}`);
        render();
      }
    });
  });

  document.querySelector<HTMLFormElement>("#proxy-form")?.addEventListener("submit", async (event) => {
    event.preventDefault();
    const form = event.currentTarget as HTMLFormElement | null;
    if (!form) return;
    const formData = new FormData(form);

    try {
      const settings = await backend<AppSettings>("save_app_settings", {
        input: {
          proxyEnabled: formData.get("proxyEnabled") === "on",
          proxyHttp: String(formData.get("proxyHttp") ?? ""),
          proxyHttps: String(formData.get("proxyHttps") ?? ""),
          proxyAll: String(formData.get("proxyAll") ?? ""),
        },
      });
      state.settings = settings;
      pushSystemLog("代理设置已保存");
      render();
    } catch (error) {
      pushSystemLog(`保存代理失败: ${String(error)}`);
      render();
    }
  });

  document.querySelector<HTMLFormElement>("#global-settings-form")?.addEventListener("submit", async (event) => {
    event.preventDefault();
    const form = event.currentTarget as HTMLFormElement | null;
    if (!form) return;
    const formData = new FormData(form);

    try {
      const settings = await backend<AppSettings>("save_app_settings", {
        input: {
          ...state.settings,
          openaiClientId: String(formData.get("openaiClientId") ?? ""),
          openaiAuthUrl: String(formData.get("openaiAuthUrl") ?? ""),
          openaiTokenUrl: String(formData.get("openaiTokenUrl") ?? ""),
          openaiRedirectUri: String(formData.get("openaiRedirectUri") ?? ""),
          openaiScope: String(formData.get("openaiScope") ?? ""),
          registrationTimeout: Number(formData.get("registrationTimeout") ?? state.settings.registrationTimeout),
          registrationMaxRetries: Number(formData.get("registrationMaxRetries") ?? state.settings.registrationMaxRetries),
          batchIntervalSeconds: Number(formData.get("batchIntervalSeconds") ?? state.settings.batchIntervalSeconds),
        },
      });
      state.settings = settings;
      pushSystemLog("全局设置已保存");
      render();
    } catch (error) {
      pushSystemLog(`保存全局设置失败: ${String(error)}`);
      render();
    }
  });

  document.querySelector<HTMLFormElement>("#cpa-form")?.addEventListener("submit", async (event) => {
    event.preventDefault();
    const form = event.currentTarget as HTMLFormElement | null;
    if (!form) return;
    const formData = new FormData(form);
    try {
      const settings = await backend<AppSettings>("save_app_settings", {
        input: {
          ...state.settings,
          cpaEnabled: formData.get("cpaEnabled") === "on",
          cpaApiUrl: String(formData.get("cpaApiUrl") ?? ""),
          cpaApiToken: String(formData.get("cpaApiToken") ?? ""),
        },
      });
      state.settings = settings;
      pushSystemLog("CPA 设置已保存");
      render();
    } catch (error) {
      pushSystemLog(`保存 CPA 设置失败: ${String(error)}`);
      render();
    }
  });

  document.querySelector("#backup-db-btn")?.addEventListener("click", async () => {
    try {
      const result = await backend<{ backupPath: string }>("backup_database");
      pushSystemLog(`数据库已备份到: ${result.backupPath}`);
      await bootstrap();
    } catch (error) {
      pushSystemLog(`备份数据库失败: ${String(error)}`);
      render();
    }
  });

  document.querySelector("#clear-tasks-btn")?.addEventListener("click", async () => {
    try {
      const result = await backend<{ message: string }>("clear_registration_tasks");
      pushSystemLog(result.message);
      await bootstrap();
    } catch (error) {
      pushSystemLog(`清理任务失败: ${String(error)}`);
      render();
    }
  });

  document.querySelector("#refresh-accounts-btn")?.addEventListener("click", () => void bootstrap());

  document.querySelector("#refresh-token-btn")?.addEventListener("click", async () => {
    const ids = selectedAccountIdsFromDom();
    if (ids.length !== 1) {
      pushSystemLog("请选择一个账号执行 Token 刷新");
      render();
      return;
    }
    try {
      const result = await backend<{ message: string }>("refresh_account_token", { accountId: ids[0] });
      pushSystemLog(result.message);
      await bootstrap();
    } catch (error) {
      pushSystemLog(`刷新 Token 失败: ${String(error)}`);
      render();
    }
  });

  document.querySelector("#validate-token-btn")?.addEventListener("click", async () => {
    const ids = selectedAccountIdsFromDom();
    if (ids.length !== 1) {
      pushSystemLog("请选择一个账号执行 Token 验证");
      render();
      return;
    }
    try {
      const result = await backend<{ message: string }>("validate_account_token", { accountId: ids[0] });
      pushSystemLog(result.message);
    } catch (error) {
      pushSystemLog(`验证 Token 失败: ${String(error)}`);
      render();
    }
  });

  document.querySelector("#batch-validate-btn")?.addEventListener("click", async () => {
    const ids = selectedAccountIdsFromDom();
    if (ids.length === 0) {
      pushSystemLog("请先选择账号");
      render();
      return;
    }
    try {
      const results = await backend<Array<{ message: string }>>("batch_validate_tokens", { input: { ids } });
      pushSystemLog(`批量验证完成，共 ${results.length} 个结果`);
      results.forEach((item) => pushSystemLog(item.message));
    } catch (error) {
      pushSystemLog(`批量验证失败: ${String(error)}`);
      render();
    }
  });

  document.querySelector("#mark-active-btn")?.addEventListener("click", async () => {
    const ids = selectedAccountIdsFromDom();
    if (ids.length === 0) {
      pushSystemLog("请先选择账号");
      render();
      return;
    }
    try {
      const result = await backend<{ message: string }>("update_accounts_status", { input: { ids, status: "active" } });
      pushSystemLog(result.message);
      state.selectedAccountIds = [];
      await bootstrap();
    } catch (error) {
      pushSystemLog(`更新账号状态失败: ${String(error)}`);
      render();
    }
  });

  document.querySelector("#mark-failed-btn")?.addEventListener("click", async () => {
    const ids = selectedAccountIdsFromDom();
    if (ids.length === 0) {
      pushSystemLog("请先选择账号");
      render();
      return;
    }
    try {
      const result = await backend<{ message: string }>("update_accounts_status", { input: { ids, status: "failed" } });
      pushSystemLog(result.message);
      state.selectedAccountIds = [];
      await bootstrap();
    } catch (error) {
      pushSystemLog(`更新账号状态失败: ${String(error)}`);
      render();
    }
  });

  document.querySelector("#delete-accounts-btn")?.addEventListener("click", async () => {
    const ids = selectedAccountIdsFromDom();
    if (ids.length === 0) {
      pushSystemLog("请先选择账号");
      render();
      return;
    }
    try {
      const result = await backend<{ message: string }>("delete_accounts", { input: { ids } });
      pushSystemLog(result.message);
      state.selectedAccountIds = [];
      await bootstrap();
    } catch (error) {
      pushSystemLog(`删除账号失败: ${String(error)}`);
      render();
    }
  });

  document.querySelector("#export-json-btn")?.addEventListener("click", async () => {
    const ids = selectedAccountIdsFromDom();
    if (ids.length === 0) {
      pushSystemLog("请先选择账号");
      render();
      return;
    }
    try {
      const result = await backend<{ filename: string; content: string }>("export_accounts", { input: { ids, format: "json" } });
      downloadTextFile(result.filename, result.content);
      pushSystemLog(`已导出 ${ids.length} 个账号为 JSON`);
    } catch (error) {
      pushSystemLog(`导出账号失败: ${String(error)}`);
      render();
    }
  });

  document.querySelector("#export-csv-btn")?.addEventListener("click", async () => {
    const ids = selectedAccountIdsFromDom();
    if (ids.length === 0) {
      pushSystemLog("请先选择账号");
      render();
      return;
    }
    try {
      const result = await backend<{ filename: string; content: string }>("export_accounts", { input: { ids, format: "csv" } });
      downloadTextFile(result.filename, result.content);
      pushSystemLog(`已导出 ${ids.length} 个账号为 CSV`);
    } catch (error) {
      pushSystemLog(`导出账号失败: ${String(error)}`);
      render();
    }
  });

  document.querySelector("#export-cpa-btn")?.addEventListener("click", async () => {
    const ids = selectedAccountIdsFromDom();
    if (ids.length === 0) {
      pushSystemLog("请先选择账号");
      render();
      return;
    }
    try {
      const result = await backend<{ filename: string; content: string }>("export_cpa_accounts", { input: { ids } });
      downloadTextFile(result.filename, result.content);
      pushSystemLog(`已导出 ${ids.length} 个账号为 CPA`);
    } catch (error) {
      pushSystemLog(`导出 CPA 失败: ${String(error)}`);
      render();
    }
  });

  document.querySelector("#upload-cpa-btn")?.addEventListener("click", async () => {
    const ids = selectedAccountIdsFromDom();
    if (ids.length === 0) {
      pushSystemLog("请先选择账号");
      render();
      return;
    }
    try {
      const result = await backend<{ message: string }>("upload_cpa_accounts", { input: { ids } });
      pushSystemLog(result.message);
      await bootstrap();
    } catch (error) {
      pushSystemLog(`上传 CPA 失败: ${String(error)}`);
      render();
    }
  });

  document.querySelectorAll<HTMLInputElement>('[data-account-id]').forEach((input) => {
    input.addEventListener("change", () => {
      state.selectedAccountIds = selectedAccountIdsFromDom();
    });
  });

  document.querySelector<HTMLFormElement>("#runner-form")?.addEventListener("submit", async (event) => {
    event.preventDefault();
    const form = event.currentTarget as HTMLFormElement | null;
    if (!form) return;
    const formData = new FormData(form);
    const serviceId = state.selectedRunServiceId ?? currentRunnableService()?.id ?? 0;
    const count = Number(formData.get("count") || 1);

    if (!serviceId) {
      pushSystemLog("请先到“设置”页保存服务，再回来执行注册");
      render();
      return;
    }

    try {
      const preview = await backend<PreviewEmail[]>("preview_emails", {
        request: {
          serviceId,
          count: state.mode === "single" ? 1 : count,
        },
      });
      state.previewEmails = preview;
      state.activeTab = "mail";
      pushSystemLog(`已预生成 ${preview.length} 个邮箱，等待确认执行`);
      render();
    } catch (error) {
      pushSystemLog(`预生成邮箱失败: ${String(error)}`);
      render();
    }
  });

  document.querySelector("#confirm-preview-btn")?.addEventListener("click", async () => {
    if (state.previewEmails.length === 0) {
      pushSystemLog("没有待确认邮箱，请先预生成");
      render();
      return;
    }

    const form = document.querySelector<HTMLFormElement>("#runner-form");
    const formData = form ? new FormData(form) : null;
    const interval = Number(formData?.get("interval") || 0);
    const serviceId = Number(formData?.get("serviceId") || currentService()?.id || 0);

    try {
      if (state.mode === "single") {
        const task = await backend<TaskSnapshot>("start_single_registration", {
          request: {
            serviceId,
            previewEmail: state.previewEmails[0],
          },
        });
        state.selectedTaskId = task.id;
        pushSystemLog(`单次注册任务已启动: ${task.id.slice(0, 8)}`);
      } else {
        const task = await backend<TaskSnapshot>("start_batch_registration", {
          request: {
            serviceId,
            count: state.previewEmails.length,
            previewEmails: state.previewEmails,
            intervalSeconds: interval,
          },
        });
        state.selectedTaskId = task.id;
        pushSystemLog(`批量注册任务已启动: ${task.id.slice(0, 8)}`);
      }

      state.previewEmails = [];
      state.activeTab = "register";
      await refreshTasks();
    } catch (error) {
      pushSystemLog(`启动任务失败: ${String(error)}`);
      render();
    }
  });

  document.querySelectorAll<HTMLElement>("[data-task-id]").forEach((element) => {
    element.addEventListener("click", () => {
      state.selectedTaskId = element.dataset.taskId ?? null;
      render();
    });
  });
}

async function bootstrap() {
  try {
    const payload = await backend<BootstrapPayload>("bootstrap_app");
    state.services = payload.services;
    state.accounts = payload.accounts;
    state.settings = payload.settings;
    state.accountStats = payload.accountStats;
    state.databaseInfo = payload.databaseInfo;
    if (
      state.selectedRunServiceId === null ||
      !state.services.some((service) => service.id === state.selectedRunServiceId && service.enabled)
    ) {
      state.selectedRunServiceId = state.services.find((service) => service.enabled)?.id ?? null;
    }
    if (!state.services.some((service) => service.service_type === state.selectedServiceType)) {
      state.selectedServiceType = state.services.some((service) => service.service_type === "gptmail")
        ? "gptmail"
        : "custom_domain";
    }
    await refreshTasks(false);
  } catch (_error) {
  }
  render();
}

async function refreshTasks(renderAfter = true) {
  try {
    const tasks = await backend<TaskSnapshot[]>("list_tasks");
    state.tasks = tasks;
    if (!state.selectedTaskId && tasks[0]) {
      state.selectedTaskId = tasks[0].id;
    }
    if (state.selectedTaskId && !tasks.some((task) => task.id === state.selectedTaskId)) {
        state.selectedTaskId = tasks[0]?.id ?? null;
    }
  } catch (_error) {
  }
  if (renderAfter) {
    render();
  }
}

function statusText(status: TaskSnapshot["status"]) {
  const map: Record<TaskSnapshot["status"], string> = {
    pending: "等待中",
    running: "运行中",
    completed: "已完成",
    failed: "失败",
    cancelled: "已取消",
  };
  return map[status];
}

function escapeHtml(input: string) {
  return input
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#039;");
}

function escapeAttr(input: string) {
  return escapeHtml(input);
}

function startPolling() {
  if (taskPollingHandle) {
    window.clearInterval(taskPollingHandle);
  }
  taskPollingHandle = window.setInterval(() => {
    void refreshTasks();
  }, 1800);
}

async function startRealtimeTaskEvents() {
  if (!isTauriRuntime || taskUnlisten) return;
  try {
    taskUnlisten = await listen<TaskSnapshot>("task://updated", (event) => {
      const updatedTask = event.payload;
      const existingIndex = state.tasks.findIndex((task) => task.id === updatedTask.id);
      if (existingIndex >= 0) {
        state.tasks.splice(existingIndex, 1, updatedTask);
      } else {
        state.tasks.unshift(updatedTask);
      }
      if (!state.selectedTaskId) {
        state.selectedTaskId = updatedTask.id;
      }
      render();
    });
  } catch {
    // fallback to polling only
  }
}

render();
installGlobalUiActions();
startPolling();
void startRealtimeTaskEvents();
void bootstrap();
