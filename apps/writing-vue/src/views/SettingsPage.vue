<template>
  <div id="settings-view" class="settings-page view active" data-writing-settings>
    <div class="hero-panel__header">
      <h2 class="hero-panel__title heading-serif">系统设置</h2>
    </div>

    <div class="hero-settings-group" aria-label="设置主面板">
      <section class="hero-panel hero-section ai-settings-panel">
        <h3 class="heading-serif">AI 与评测</h3>
        <p class="hero-panel__muted">配置模型、提示词与温度。API Key 保存在系统密钥环，不会进入普通备份。</p>
        <div class="hero-settings-actions">
          <button class="btn btn-brand hero-btn" type="button" data-settings-open="api" @click="openSettingsDetail('api')">
            API 配置
          </button>
          <button class="btn hero-btn" type="button" data-settings-open="prompts" @click="openSettingsDetail('prompts')">
            提示词
          </button>
          <button class="btn hero-btn" type="button" data-settings-open="model" @click="openSettingsDetail('model')">
            模型参数
          </button>
          <button class="btn hero-btn" type="button" data-settings-open="about" @click="openSettingsDetail('about')">
            关于
          </button>
        </div>
        <div class="settings-stat-row">
          <span class="settings-badge">{{ enabledConfigCount }} 个已启用</span>
          <span class="settings-badge settings-badge--muted">共 {{ totalConfigCount }} 个配置</span>
          <span class="settings-badge settings-badge--muted">{{ promptEntries.length }} 个提示词版本</span>
        </div>
      </section>

      <section class="hero-panel hero-section data-management-panel">
        <h3 class="heading-serif">本机数据备份</h3>
        <p class="hero-panel__muted">
          完整备份由 Rust 写入应用 backups 目录，包含练习记录与设置元数据；不含明文 API Key。
        </p>
        <div class="hero-settings-actions">
          <button class="btn btn-brand hero-btn data-mgmt-btn" id="create-backup-btn" type="button" :disabled="backupBusy" @click="createFullAppBackup">
            {{ backupBusy ? '处理中…' : '备份全部数据' }}
          </button>
          <button class="btn hero-btn data-mgmt-btn" id="restore-backup-btn" type="button" :disabled="backupBusy" @click="restoreFullAppBackup">
            从备份恢复…
          </button>
          <button class="btn hero-btn data-mgmt-btn" id="backup-list-btn" type="button" :disabled="backupBusy" @click="showNativeBackupList">
            备份列表
          </button>
        </div>
        <div v-if="lastBackupPath" class="settings-backup-result hero-surface" role="status">
          <div class="settings-backup-result__label">最近完整备份已写入</div>
          <div class="settings-backup-result__path settings-path-clip">{{ lastBackupPath }}</div>
          <p v-if="backupsPath" class="settings-backup-result__hint hero-panel__muted">备份目录：{{ backupsPath }}</p>
        </div>
        <p v-else-if="backupsPath" class="hero-panel__muted settings-backup-path">备份目录：{{ backupsPath }}</p>
      </section>

      <section class="hero-panel hero-section system-management-panel">
        <h3 class="heading-serif">系统与外观</h3>
        <p class="hero-panel__muted">统一 Liquid Glass 视觉、写作题库入口、引导与原生更新工具。</p>
        <div class="hero-settings-actions">
          <button class="btn btn-warning hero-btn hero-btn--warn" id="load-library-btn" type="button" @click="openWritingTopicLibrary">
            打开写作题库
          </button>
          <p class="visual-system-note" role="status">Liquid Glass 视觉已统一</p>
          <button class="btn btn-warning hero-btn hero-btn--warn" id="show-onboarding-btn" type="button" @click="startOnboardingTour">
            显示引导
          </button>
          <button class="btn btn-warning hero-btn hero-btn--warn" id="library-config-btn" type="button" data-action="library-config" @click="openWritingLibraryConfig">
            提示词设置
          </button>
          <button class="btn btn-warning hero-btn hero-btn--warn" id="check-updates-btn" type="button" data-update-action="open-modal" @click="openUpdateManager">
            检查更新
          </button>
          <button class="btn hero-btn" type="button" @click="openSettingsDetail('data')">
            历史保留上限
          </button>
        </div>
      </section>

      <section class="hero-panel hero-section system-info-panel">
        <h3 class="heading-serif">系统信息</h3>
        <div class="hero-surface settings-system-info system-info-surface">
          <div class="settings-system-info__status system-info-status">{{ topicLibraryStatus }}</div>
          <div class="settings-system-metrics">
            <div><span>题目总数</span><strong id="total-exams">{{ topicLibraryStats.total }}</strong></div>
            <div><span>Task 1</span><strong id="html-exams">{{ topicLibraryStats.task1 }}</strong></div>
            <div><span>Task 2</span><strong id="pdf-exams">{{ topicLibraryStats.task2 }}</strong></div>
          </div>
          <p class="hero-panel__muted">最近更新：<span id="last-update">{{ topicLibraryStats.lastUpdate }}</span></p>
          <details class="settings-technical-details">
            <summary>技术详情</summary>
            <dl>
              <div><dt>桌面宿主</dt><dd>{{ hostName }}</dd></div>
              <div><dt>Tauri 版本</dt><dd>{{ tauriVersion }}</dd></div>
              <div><dt>数据目录</dt><dd class="settings-path-clip">{{ userDataPath || pathsLoadingLabel }}</dd></div>
              <div><dt>备份目录</dt><dd class="settings-path-clip">{{ backupsPath || (pathsResolved ? '未获取' : '加载中...') }}</dd></div>
            </dl>
          </details>
        </div>
        <div class="settings-credit">
          <a href="https://docs.qq.com/doc/DSXZhWUtqeVN0d1ZT" target="_blank" rel="noopener noreferrer" class="inline-hover-link">问题反馈</a>
          <a href="https://github.com/sallowayma-git" target="_blank" rel="noopener noreferrer">Salloway呈现</a>
        </div>
      </section>
    </div>

    <div v-if="globalMessage.message" :class="['inline-message', `inline-message-${globalMessage.type}`]">
      {{ globalMessage.message }}
    </div>

    <div
      v-if="settingsDetailOpen"
      class="settings-detail-modal"
      role="dialog"
      aria-modal="true"
      aria-label="写作设置明细"
      @click.self="hideSettingsDetail"
    >
      <section class="settings-detail-panel hero-panel hero-section">
        <div class="settings-detail-head">
          <div>
            <p class="settings-detail-eyebrow">{{ activeTabMeta.kicker }}</p>
            <h3 class="heading-serif">{{ activeTabMeta.title }}</h3>
            <p>{{ activeTabMeta.description }}</p>
          </div>
          <button class="settings-detail-close" type="button" aria-label="关闭写作设置" @click="hideSettingsDetail">×</button>
        </div>

        <div class="settings-tabs" role="tablist" aria-label="写作设置分类">
          <button
            v-for="tab in tabs"
            :key="tab.key"
            type="button"
            :class="['settings-tab', { active: activeTab === tab.key }]"
            @click="activeTab = tab.key"
          >
            <span class="settings-tab__icon" v-html="tab.icon"></span>
            {{ tab.label }}
          </button>
        </div>

      <!-- API 配置 -->
        <section v-if="activeTab === 'api'" class="settings-panel">
          <div class="settings-panel__head">
            <div>
              <h3>评测通道</h3>
              <p>默认配置必须启用且本机可读取 API Key。完整备份只恢复引用：同一设备且本机凭据记录仍在时可复用，换设备必须重新填写 Key。</p>
            </div>
            <div class="settings-badges">
              <span class="settings-badge">{{ totalConfigCount }} 个配置</span>
              <span class="settings-badge settings-badge--success">{{ enabledConfigCount }} 个启用</span>
            </div>
          </div>
          <div v-if="sectionMessages.api.message" :class="['inline-message', `inline-message-${sectionMessages.api.type}`]">
            {{ sectionMessages.api.message }}
          </div>
          <div v-if="apiLoading" class="settings-loading">加载配置中...</div>
          <div v-else-if="!apiConfigs.length" class="settings-empty">暂无 API 配置。</div>
          <div v-else class="settings-list">
            <div v-for="item in apiConfigs" :key="item.id" class="settings-list__row">
              <div class="settings-list__main">
                <div class="settings-list__title">
                  <strong>{{ item.config_name }}</strong>
                  <span v-if="item.is_default" class="settings-badge settings-badge--accent">默认</span>
                  <span :class="['settings-badge', item.is_enabled ? 'settings-badge--success' : 'settings-badge--muted']">
                    {{ item.is_enabled ? '启用' : '禁用' }}
                  </span>
                  <span v-if="!item.has_secret" class="settings-badge settings-badge--muted">需重新填写 Key</span>
                </div>
                <div class="settings-list__meta">
                  <span>{{ item.provider }}</span>
                  <span>{{ item.default_model }}</span>
                </div>
              </div>
              <div class="settings-actions">
                <button class="btn-text" type="button" @click="editConfig(item)">编辑</button>
                <button
                  class="btn-text"
                  type="button"
                  :disabled="testingConfigId === item.id || !item.has_secret"
                  :title="item.has_secret ? '' : '此设备没有该配置的 API Key，请先编辑并重新填写'"
                  @click="testConfig(item.id)"
                >
                    {{ testingConfigId === item.id ? '测试中' : '测试' }}
                </button>
                <button
                    class="btn-text"
                    type="button"
                    :disabled="item.is_default || !item.is_enabled || !item.has_secret"
                    :title="item.is_default ? '当前已是默认配置' : (!item.is_enabled ? '禁用配置不能设为默认' : (!item.has_secret ? '请先在此设备重新填写 API Key' : ''))"
                    @click="setDefaultConfig(item.id)"
                  >
                    设默认
                </button>
                <button
                    class="btn-text"
                    type="button"
                    :disabled="isToggleBlocked(item)"
                    :title="getToggleBlockedReason(item)"
                    @click="toggleConfig(item.id)"
                  >
                    {{ item.is_enabled ? '禁用' : '启用' }}
                </button>
                <button
                    class="btn-text danger"
                    type="button"
                    :disabled="isDeleteBlocked(item)"
     #���Kh��춻�q�^t          temperature: 0.1,
        };
        let body = agent_request_body("fake-model", &request);
        assert!(body.get("response_format").is_none());
        assert_eq!(body["tool_choice"], "auto");
        assert_eq!(body["messages"][1]["tool_call_id"], "call-1");
        assert_eq!(body["tools"][0]["function"]["name"], "read_file");
    }

    #[test]
    fn parses_null_content_and_multiple_tool_calls() {
        let envelope: AgentChatResponse = serde_json::from_value(json!({
            "id":"request-1",
            "model":"provider-model",
            "choices":[{"message":{
                "content":null,
                "tool_calls":[
                    {"id":"call-1","type":"function","function":{"name":"read_file","arguments":"{\"path\":\"a.txt\"}"}},
                    {"id":"call-2","type":"function","function":{"name":"read_file","arguments":"{\"path\":\"b.txt\"}"}}
                ]
            }}],
            "usage":{"prompt_tokens":3,"completion_tokens":4}
        }))
        .unwrap();
        let response = parse_agent_response(envelope, "fallback", 5).unwrap();
        assert!(response.content.is_none());
        assert_eq!(response.tool_calls.len(), 2);
        assert_eq!(response.tool_calls[1].name, "read_file");
        assert_eq!(response.model, "provider-model");
        assert_eq!(response.usage.unwrap().output_tokens, 4);
    }

    #[test]
    fn rejects_agent_response_without_choices() {
        let envelope: AgentChatResponse = serde_json::from_value(json!({
            "choices":[]
        }))
        .unwrap();
        let error = parse_agent_response(envelope, "fallback", 0).unwrap_err();
        assert!(error.message.contains("no choices"));
    }
}
