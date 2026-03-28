import { useState, useEffect, useMemo } from 'react';
import { apiFetch } from '@/lib/api';
import { Save, Plus, Trash2, ChevronDown, ChevronUp, Search, RefreshCw } from 'lucide-react';
import AddToolDialog from '@/components/AddToolDialog';

interface ModelInfo {
  id: string;
  name: string;
}

interface ToolDef {
  id: string;
  name: string;
  description?: string;
  category: string;
  enabled: boolean;
  config?: Record<string, unknown>;
  schema?: Record<string, unknown>;
}

interface ScanDir {
  path: string;
  total_photos: number;
  embedded_photos: number;
}

interface SystemStatus {
  total_photos: number;
  embedded_photos: number;
  scan_dirs: ScanDir[];
}

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section className="border border-border rounded-xl p-6 space-y-4">
      <h2 className="text-lg font-semibold">{title}</h2>
      {children}
    </section>
  );
}

function InputField({
  label,
  value,
  onChange,
  type = 'text',
  placeholder,
  hint,
}: {
  label: string;
  value: string;
  onChange: (v: string) => void;
  type?: string;
  placeholder?: string;
  hint?: string;
}) {
  return (
    <div>
      <label className="block text-sm font-medium mb-1 text-muted-foreground">{label}</label>
      <input
        type={type}
        value={value}
        onChange={(e) => onChange(e.target.value)}
        placeholder={placeholder}
        className="w-full px-3 py-2 rounded-lg border border-input bg-card text-foreground focus:outline-none focus:ring-2 focus:ring-ring text-sm"
      />
      {hint && (
        <p className="mt-1 text-xs text-muted-foreground font-mono truncate" title={hint}>{hint}</p>
      )}
    </div>
  );
}

/** Strip trailing slashes from a base URL */
function normalizeUrl(url: string): string {
  return url.replace(/\/+$/, '');
}

/** Build the full endpoint URL that will actually be called */
function resolveEmbeddingUrl(baseUrl: string, model: string): string {
  if (!baseUrl) return '';
  const base = normalizeUrl(baseUrl);
  const m = model || '<model>';
  return `${base}/v1/models/${m}:embedContent`;
}

function resolveAgentUrl(baseUrl: string, provider: string): string {
  if (!baseUrl) return '';
  const base = normalizeUrl(baseUrl);
  switch (provider) {
    case 'anthropic':
      return `${base}/v1/messages`;
    case 'google':
      return `${base}/v1beta/models/<model>:generateContent`;
    case 'openai':
      return `${base}/v1/chat/completions`;
    case 'openai_compat':
      return `${base}/v1/responses`;
    default:
      return `${base}/v1/chat/completions`;
  }
}

export default function SettingsPage() {
  // Embedding model config
  const [embeddingUrl, setEmbeddingUrl] = useState('');
  const [embeddingKey, setEmbeddingKey] = useState('');
  const [embeddingModels, setEmbeddingModels] = useState<ModelInfo[]>([]);
  const [selectedEmbeddingModel, setSelectedEmbeddingModel] = useState('');

  // Agent model config
  const [agentProvider, setAgentProvider] = useState('openai');
  const [agentUrl, setAgentUrl] = useState('');
  const [agentKey, setAgentKey] = useState('');
  const [agentModels, setAgentModels] = useState<ModelInfo[]>([]);
  const [selectedAgentModel, setSelectedAgentModel] = useState('');
  const [manualModelName, setManualModelName] = useState('');

  // Tools
  const [tools, setTools] = useState<ToolDef[]>([]);
  const [toolSearch, setToolSearch] = useState('');
  const [expandedTool, setExpandedTool] = useState<string | null>(null);
  const [showAddTool, setShowAddTool] = useState(false);

  // Scan dirs
  const [scanDirs, setScanDirs] = useState<string[]>([]);
  const [newScanDir, setNewScanDir] = useState('');

  // Status
  const [status, setStatus] = useState<SystemStatus | null>(null);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    loadSettings();
  }, []);

  const loadSettings = async () => {
    try {
      const cfg = await apiFetch<Record<string, unknown>>('/settings');
      if (cfg.embedding_url) setEmbeddingUrl(cfg.embedding_url as string);
      if (cfg.embedding_key) setEmbeddingKey(cfg.embedding_key as string);
      if (cfg.embedding_model) setSelectedEmbeddingModel(cfg.embedding_model as string);
      if (cfg.agent_provider) setAgentProvider(cfg.agent_provider as string);
      if (cfg.agent_url) setAgentUrl(cfg.agent_url as string);
      if (cfg.agent_key) setAgentKey(cfg.agent_key as string);
      if (cfg.agent_model) setSelectedAgentModel(cfg.agent_model as string);
      if (cfg.scan_dirs) setScanDirs(cfg.scan_dirs as string[]);
    } catch { /* initial load, might 404 */ }

    try {
      const t = await apiFetch<ToolDef[]>('/tools');
      setTools(t);
    } catch { /* no tools yet */ }

    try {
      const s = await apiFetch<SystemStatus>('/status');
      setStatus(s);
    } catch { /* ok */ }
  };

  const saveSettings = async () => {
    setSaving(true);
    try {
      await apiFetch('/settings', {
        method: 'PUT',
        body: JSON.stringify({
          embedding_url: embeddingUrl,
          embedding_key: embeddingKey,
          embedding_model: selectedEmbeddingModel,
          agent_provider: agentProvider,
          agent_url: agentUrl,
          agent_key: agentKey,
          agent_model: selectedAgentModel,
          scan_dirs: scanDirs,
        }),
      });
    } finally {
      setSaving(false);
    }
  };

  const fetchEmbeddingModels = async () => {
    try {
      const models = await apiFetch<ModelInfo[]>('/settings/embedding-models', {
        method: 'POST',
        body: JSON.stringify({ url: embeddingUrl, key: embeddingKey }),
      });
      setEmbeddingModels(models);
    } catch { /* show error */ }
  };

  const fetchAgentModels = async () => {
    try {
      const models = await apiFetch<ModelInfo[]>('/settings/agent-models', {
        method: 'POST',
        body: JSON.stringify({ provider: agentProvider, url: agentUrl, key: agentKey }),
      });
      setAgentModels(models);
    } catch { /* show error */ }
  };

  const toggleTool = async (toolId: string, enabled: boolean) => {
    await apiFetch(`/tools/${toolId}`, {
      method: 'PATCH',
      body: JSON.stringify({ enabled }),
    });
    setTools((prev) => prev.map((t) => (t.id === toolId ? { ...t, enabled } : t)));
  };

  const filteredTools = tools.filter(
    (t) =>
      t.name.toLowerCase().includes(toolSearch.toLowerCase()) ||
      t.description?.toLowerCase().includes(toolSearch.toLowerCase()) ||
      t.category.toLowerCase().includes(toolSearch.toLowerCase())
  );

  const embeddingUrlHint = useMemo(
    () => resolveEmbeddingUrl(embeddingUrl, selectedEmbeddingModel),
    [embeddingUrl, selectedEmbeddingModel]
  );

  const agentUrlHint = useMemo(
    () => resolveAgentUrl(agentUrl, agentProvider),
    [agentUrl, agentProvider]
  );

  return (
    <div className="max-w-3xl mx-auto px-4 py-8 space-y-6">
      <h1 className="text-2xl font-bold">Settings</h1>

      {/* System Status */}
      {status && (
        <Section title="System Status">
          <div className="grid grid-cols-2 gap-4 text-sm">
            <div className="bg-muted rounded-lg p-3">
              <p className="text-muted-foreground">Total Photos</p>
              <p className="text-2xl font-bold">{status.total_photos}</p>
            </div>
            <div className="bg-muted rounded-lg p-3">
              <p className="text-muted-foreground">Embedded</p>
              <p className="text-2xl font-bold">{status.embedded_photos}</p>
            </div>
          </div>
        </Section>
      )}

      {/* Scan Directories */}
      <Section title="Scan Directories">
        <div className="space-y-2">
          {scanDirs.map((dir, i) => (
            <div key={i} className="flex items-center gap-2">
              <span className="flex-1 text-sm bg-muted rounded-lg px-3 py-2">{dir}</span>
              <button
                onClick={() => setScanDirs((prev) => prev.filter((_, j) => j !== i))}
                className="p-2 text-destructive hover:opacity-75"
              >
                <Trash2 className="w-4 h-4" />
              </button>
            </div>
          ))}
        </div>
        <div className="flex gap-2">
          <input
            type="text"
            value={newScanDir}
            onChange={(e) => setNewScanDir(e.target.value)}
            placeholder="/path/to/photos"
            className="flex-1 px-3 py-2 rounded-lg border border-input bg-card text-foreground text-sm focus:outline-none focus:ring-2 focus:ring-ring"
          />
          <button
            onClick={() => {
              if (newScanDir.trim()) {
                setScanDirs((prev) => [...prev, newScanDir.trim()]);
                setNewScanDir('');
              }
            }}
            className="px-3 py-2 rounded-lg bg-primary text-primary-foreground text-sm"
          >
            <Plus className="w-4 h-4" />
          </button>
        </div>
      </Section>

      {/* Embedding Model */}
      <Section title="Embedding Model">
        <InputField label="API URL" value={embeddingUrl} onChange={setEmbeddingUrl} placeholder="https://generativelanguage.googleapis.com" hint={embeddingUrlHint} />
        <InputField label="API Key" value={embeddingKey} onChange={setEmbeddingKey} type="password" placeholder="Your API key" />
        <div className="flex gap-2 items-end">
          <div className="flex-1">
            <label className="block text-sm font-medium mb-1 text-muted-foreground">Model</label>
            <select
              value={selectedEmbeddingModel}
              onChange={(e) => setSelectedEmbeddingModel(e.target.value)}
              className="w-full px-3 py-2 rounded-lg border border-input bg-card text-foreground text-sm focus:outline-none focus:ring-2 focus:ring-ring"
            >
              <option value="">Select a model...</option>
              {embeddingModels.map((m) => (
                <option key={m.id} value={m.id}>{m.name}</option>
              ))}
            </select>
          </div>
          <button
            onClick={fetchEmbeddingModels}
            className="px-4 py-2 rounded-lg border border-input bg-card text-sm hover:bg-muted"
          >
            Fetch Models
          </button>
        </div>
      </Section>

      {/* Agent Model */}
      <Section title="Agent Model">
        <div>
          <label className="block text-sm font-medium mb-1 text-muted-foreground">Provider</label>
          <select
            value={agentProvider}
            onChange={(e) => setAgentProvider(e.target.value)}
            className="w-full px-3 py-2 rounded-lg border border-input bg-card text-foreground text-sm focus:outline-none focus:ring-2 focus:ring-ring"
          >
            <option value="anthropic">Anthropic (/v1/messages)</option>
            <option value="google">Google (/v1beta/generateContent)</option>
            <option value="openai">OpenAI (/v1/chat/completions)</option>
            <option value="openai_compat">OpenAI Compatible (/v1/responses)</option>
          </select>
        </div>
        <InputField label="API URL" value={agentUrl} onChange={setAgentUrl} placeholder="https://api.openai.com" hint={agentUrlHint} />
        <InputField label="API Key" value={agentKey} onChange={setAgentKey} type="password" placeholder="Your API key" />
        <div className="flex gap-2 items-end">
          <div className="flex-1">
            <label className="block text-sm font-medium mb-1 text-muted-foreground">Model</label>
            <select
              value={selectedAgentModel}
              onChange={(e) => setSelectedAgentModel(e.target.value)}
              className="w-full px-3 py-2 rounded-lg border border-input bg-card text-foreground text-sm focus:outline-none focus:ring-2 focus:ring-ring"
            >
              <option value="">Select a model...</option>
              {agentModels.map((m) => (
                <option key={m.id} value={m.id}>{m.name}</option>
              ))}
            </select>
          </div>
          <button
            onClick={fetchAgentModels}
            className="px-4 py-2 rounded-lg border border-input bg-card text-sm hover:bg-muted"
          >
            Fetch Models
          </button>
        </div>
        <div className="flex gap-2">
          <input
            type="text"
            value={manualModelName}
            onChange={(e) => setManualModelName(e.target.value)}
            placeholder="Or manually enter model name..."
            className="flex-1 px-3 py-2 rounded-lg border border-input bg-card text-foreground text-sm focus:outline-none focus:ring-2 focus:ring-ring"
          />
          <button
            onClick={() => {
              if (manualModelName.trim()) {
                setAgentModels((prev) => [...prev, { id: manualModelName.trim(), name: manualModelName.trim() }]);
                setSelectedAgentModel(manualModelName.trim());
                setManualModelName('');
              }
            }}
            className="px-3 py-2 rounded-lg bg-primary text-primary-foreground text-sm"
          >
            Add
          </button>
        </div>
      </Section>

      {/* Tools */}
      <Section title="Tools">
        <div className="relative">
          <Search className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-muted-foreground" />
          <input
            type="text"
            value={toolSearch}
            onChange={(e) => setToolSearch(e.target.value)}
            placeholder="Search tools..."
            className="w-full pl-9 pr-3 py-2 rounded-lg border border-input bg-card text-foreground text-sm focus:outline-none focus:ring-2 focus:ring-ring"
          />
        </div>
        <div className="space-y-2">
          {filteredTools.map((tool) => (
            <div key={tool.id} className="border border-border rounded-lg">
              <div
                className="flex items-center justify-between px-4 py-3 cursor-pointer hover:bg-muted/50"
                onClick={() => setExpandedTool(expandedTool === tool.id ? null : tool.id)}
              >
                <div className="flex items-center gap-3">
                  <span
                    className={`inline-block px-2 py-0.5 rounded text-xs font-medium ${
                      tool.category === 'builtin'
                        ? 'bg-primary/10 text-primary'
                        : 'bg-accent text-accent-foreground'
                    }`}
                  >
                    {tool.category}
                  </span>
                  <span className="font-medium text-sm">{tool.name}</span>
                </div>
                <div className="flex items-center gap-3">
                  <label className="relative inline-flex items-center cursor-pointer" onClick={(e) => e.stopPropagation()}>
                    <input
                      type="checkbox"
                      checked={tool.enabled}
                      onChange={(e) => toggleTool(tool.id, e.target.checked)}
                      className="sr-only peer"
                    />
                    <div className="w-9 h-5 bg-muted rounded-full peer peer-checked:bg-primary after:content-[''] after:absolute after:top-0.5 after:left-[2px] after:bg-white after:rounded-full after:h-4 after:w-4 after:transition-all peer-checked:after:translate-x-full" />
                  </label>
                  {expandedTool === tool.id ? (
                    <ChevronUp className="w-4 h-4 text-muted-foreground" />
                  ) : (
                    <ChevronDown className="w-4 h-4 text-muted-foreground" />
                  )}
                </div>
              </div>
              {expandedTool === tool.id && (
                <div className="px-4 pb-3 text-sm text-muted-foreground border-t border-border pt-3 space-y-2">
                  {tool.description && <p>{tool.description}</p>}
                  {tool.schema && (
                    <pre className="bg-muted rounded-lg p-3 text-xs overflow-x-auto">
                      {JSON.stringify(tool.schema, null, 2)}
                    </pre>
                  )}
                </div>
              )}
            </div>
          ))}
        </div>
        <button
          onClick={() => setShowAddTool(true)}
          className="w-full flex items-center justify-center gap-2 px-3 py-2 rounded-lg border border-dashed border-border text-sm text-muted-foreground hover:bg-muted hover:text-foreground transition-colors"
        >
          <Plus className="w-4 h-4" /> Add Custom Tool
        </button>
      </Section>

      {/* Save */}
      <div className="flex gap-3">
        <button
          onClick={saveSettings}
          disabled={saving}
          className="flex-1 flex items-center justify-center gap-2 px-6 py-3 rounded-xl bg-primary text-primary-foreground font-medium hover:opacity-90 disabled:opacity-50"
        >
          <Save className="w-5 h-5" />
          {saving ? 'Saving...' : 'Save Settings'}
        </button>
        <button
          onClick={async () => {
            await saveSettings();
            await apiFetch('/scan', { method: 'POST' });
          }}
          className="flex items-center gap-2 px-6 py-3 rounded-xl border border-input bg-card font-medium hover:bg-muted"
        >
          <RefreshCw className="w-5 h-5" />
          Scan & Embed
        </button>
      </div>

      <AddToolDialog
        open={showAddTool}
        onClose={() => setShowAddTool(false)}
        onAdded={loadSettings}
      />
    </div>
  );
}
