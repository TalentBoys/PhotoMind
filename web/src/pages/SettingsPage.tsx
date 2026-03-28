import { useState, useEffect, useMemo, useRef, useCallback } from 'react';
import { apiFetch } from '@/lib/api';
import { Plus, Trash2, ChevronDown, ChevronUp, Search, RefreshCw, FolderOpen, Pause, Play, Square, ShieldCheck } from 'lucide-react';
import AddToolDialog from '@/components/AddToolDialog';
import DirBrowserDialog from '@/components/DirBrowserDialog';

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

interface ScanProgress {
  phase: string;
  total: number;
  processed: number;
  failed: number;
  vision_calls: number;
  vision_tokens: number;
  embed_calls: number;
  embed_tokens: number;
  current_file: string;
  error: string | null;
  started_at: number | null;
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
  onBlur,
  type = 'text',
  placeholder,
  hint,
}: {
  label: string;
  value: string;
  onChange: (v: string) => void;
  onBlur?: () => void;
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
        onBlur={onBlur}
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
  const [embeddingDimension, setEmbeddingDimension] = useState(768);

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
  const [autoApproveTools, setAutoApproveTools] = useState<Set<string>>(() => {
    try {
      const saved = localStorage.getItem('photomind_auto_approve_tools');
      return saved ? new Set(JSON.parse(saved)) : new Set();
    } catch { return new Set(); }
  });

  // Scan dirs
  const [scanDirs, setScanDirs] = useState<string[]>([]);
  const [newScanDir, setNewScanDir] = useState('');
  const [showDirBrowser, setShowDirBrowser] = useState(false);
  const [scanDirError, setScanDirError] = useState('');

  // Test results
  const [embeddingTest, setEmbeddingTest] = useState<{ ok: boolean; message?: string; error?: string } | null>(null);
  const [testingEmbedding, setTestingEmbedding] = useState(false);
  const [agentTest, setAgentTest] = useState<{ ok: boolean; message?: string; error?: string } | null>(null);
  const [testingAgent, setTestingAgent] = useState(false);

  // Image-to-Text model config
  const [i2tEnabled, setI2tEnabled] = useState(false);
  const [i2tProvider, setI2tProvider] = useState('openai');
  const [i2tUrl, setI2tUrl] = useState('');
  const [i2tKey, setI2tKey] = useState('');
  const [i2tModels, setI2tModels] = useState<ModelInfo[]>([]);
  const [selectedI2tModel, setSelectedI2tModel] = useState('');
  const [manualI2tModel, setManualI2tModel] = useState('');
  const [i2tTest, setI2tTest] = useState<{ ok: boolean; message?: string; error?: string } | null>(null);
  const [testingI2t, setTestingI2t] = useState(false);

  // Status
  const [addingDir, setAddingDir] = useState(false);
  const [status, setStatus] = useState<SystemStatus | null>(null);

  // Embedding concurrency
  const [embeddingConcurrency, setEmbeddingConcurrency] = useState(1);

  // Scan progress
  const [scanProgress, setScanProgress] = useState<ScanProgress | null>(null);
  const pollRef = useRef<ReturnType<typeof setInterval> | null>(null);

  const isRunning = scanProgress && ['scanning', 'embedding', 'paused', 'pausing', 'stopping'].includes(scanProgress.phase);

  const pollProgress = useCallback(async () => {
    try {
      const p = await apiFetch<ScanProgress>('/scan/progress');
      setScanProgress(p);
      // Stop polling when done/error/idle
      if (!['scanning', 'embedding', 'paused', 'pausing', 'stopping'].includes(p.phase)) {
        if (pollRef.current) {
          clearInterval(pollRef.current);
          pollRef.current = null;
        }
        // Refresh status counts
        try {
          const s = await apiFetch<SystemStatus>('/status');
          setStatus(s);
        } catch { /* ok */ }
      }
    } catch { /* ok */ }
  }, []);

  const startPolling = useCallback(() => {
    if (pollRef.current) return;
    pollProgress();
    pollRef.current = setInterval(pollProgress, 1000);
  }, [pollProgress]);

  useEffect(() => {
    return () => {
      if (pollRef.current) clearInterval(pollRef.current);
    };
  }, []);

  useEffect(() => {
    loadSettings();
  }, []);

  const loadSettings = async () => {
    try {
      const cfg = await apiFetch<Record<string, unknown>>('/settings');
      if (cfg.embedding_url) setEmbeddingUrl(cfg.embedding_url as string);
      if (cfg.embedding_key) setEmbeddingKey(cfg.embedding_key as string);
      if (cfg.embedding_model) {
        const em = cfg.embedding_model as string;
        setSelectedEmbeddingModel(em);
        setEmbeddingModels((prev) => prev.some((m) => m.id === em) ? prev : [...prev, { id: em, name: em }]);
      }
      if (cfg.embedding_dimension) setEmbeddingDimension(cfg.embedding_dimension as number);
      if (cfg.embedding_concurrency) setEmbeddingConcurrency(cfg.embedding_concurrency as number);
      if (cfg.agent_provider) setAgentProvider(cfg.agent_provider as string);
      if (cfg.agent_url) setAgentUrl(cfg.agent_url as string);
      if (cfg.agent_key) setAgentKey(cfg.agent_key as string);
      if (cfg.agent_model) {
        const am = cfg.agent_model as string;
        setSelectedAgentModel(am);
        setAgentModels((prev) => prev.some((m) => m.id === am) ? prev : [...prev, { id: am, name: am }]);
      }
      if (cfg.scan_dirs) setScanDirs(cfg.scan_dirs as string[]);
      if (cfg.i2t_enabled) setI2tEnabled(cfg.i2t_enabled as boolean);
      if (cfg.i2t_provider) setI2tProvider(cfg.i2t_provider as string);
      if (cfg.i2t_url) setI2tUrl(cfg.i2t_url as string);
      if (cfg.i2t_key) setI2tKey(cfg.i2t_key as string);
      if (cfg.i2t_model) {
        const im = cfg.i2t_model as string;
        setSelectedI2tModel(im);
        setI2tModels((prev) => prev.some((m) => m.id === im) ? prev : [...prev, { id: im, name: im }]);
      }
    } catch { /* initial load, might 404 */ }

    try {
      const t = await apiFetch<ToolDef[]>('/tools');
      setTools(t);
    } catch { /* no tools yet */ }

    try {
      const s = await apiFetch<SystemStatus>('/status');
      setStatus(s);
    } catch { /* ok */ }

    // Check if a scan is already running
    try {
      const p = await apiFetch<ScanProgress>('/scan/progress');
      setScanProgress(p);
      if (['scanning', 'embedding', 'paused', 'pausing', 'stopping'].includes(p.phase)) {
        startPolling();
      }
    } catch { /* ok */ }
  };

  const saveField = async (key: string, value: unknown) => {
    await apiFetch('/settings', {
      method: 'PUT',
      body: JSON.stringify({ [key]: value }),
    });
  };

  const saveScanDirs = async (dirs: string[]) => {
    await saveField('scan_dirs', dirs);
  };

  const addScanDir = async () => {
    const dir = newScanDir.trim();
    if (!dir || addingDir) return;
    if (scanDirs.includes(dir)) {
      setScanDirError('Directory already added');
      return;
    }
    setAddingDir(true);
    try {
      await apiFetch('/browse-dirs', {
        method: 'POST',
        body: JSON.stringify({ path: dir }),
      });
      const updated = [...scanDirs, dir];
      await saveScanDirs(updated);
      setScanDirs(updated);
      setNewScanDir('');
      setScanDirError('');
    } catch {
      setScanDirError('Invalid path or directory not accessible');
    } finally {
      setAddingDir(false);
    }
  };

  const removeScanDir = async (index: number) => {
    const updated = scanDirs.filter((_, j) => j !== index);
    setScanDirs(updated);
    await saveScanDirs(updated);
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

  const fetchI2tModels = async () => {
    try {
      const models = await apiFetch<ModelInfo[]>('/settings/agent-models', {
        method: 'POST',
        body: JSON.stringify({ provider: i2tProvider, url: i2tUrl, key: i2tKey }),
      });
      setI2tModels(models);
    } catch { /* show error */ }
  };

  const toggleTool = async (toolId: string, enabled: boolean) => {
    await apiFetch(`/tools/${toolId}`, {
      method: 'PATCH',
      body: JSON.stringify({ enabled }),
    });
    setTools((prev) => prev.map((t) => (t.id === toolId ? { ...t, enabled } : t)));
  };

  /** Convert tool DB id (builtin:search_photos) to LLM name (builtin_search_photos) */
  const toolIdToName = (id: string) => id.replace(':', '_');

  const isAutoApproved = (toolId: string) => autoApproveTools.has(toolIdToName(toolId));

  const toggleAutoApprove = (toolId: string) => {
    const name = toolIdToName(toolId);
    const next = new Set(autoApproveTools);
    if (next.has(name)) {
      next.delete(name);
    } else {
      next.add(name);
    }
    setAutoApproveTools(next);
    localStorage.setItem('photomind_auto_approve_tools', JSON.stringify([...next]));
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
                onClick={() => removeScanDir(i)}
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
            onChange={(e) => { setNewScanDir(e.target.value); setScanDirError(''); }}
            onKeyDown={(e) => { if (e.key === 'Enter') addScanDir(); }}
            placeholder="/path/to/photos"
            className={`flex-1 px-3 py-2 rounded-lg border bg-card text-foreground text-sm focus:outline-none focus:ring-2 focus:ring-ring ${scanDirError ? 'border-destructive' : 'border-input'}`}
          />
          <button
            onClick={addScanDir}
            className="px-3 py-2 rounded-lg bg-primary text-primary-foreground text-sm"
          >
            <Plus className="w-4 h-4" />
          </button>
          <button
            onClick={() => setShowDirBrowser(true)}
            className="px-3 py-2 rounded-lg border border-input bg-card text-sm hover:bg-muted"
            title="Browse directories"
          >
            <FolderOpen className="w-4 h-4" />
          </button>
        </div>
        {scanDirError && (
          <p className="text-xs text-destructive">{scanDirError}</p>
        )}
      </Section>

      {/* Embedding Model */}
      <Section title="Embedding Model">
        <InputField label="API URL" value={embeddingUrl} onChange={setEmbeddingUrl} onBlur={() => saveField('embedding_url', embeddingUrl)} placeholder="https://generativelanguage.googleapis.com" hint={embeddingUrlHint} />
        <InputField label="API Key" value={embeddingKey} onChange={setEmbeddingKey} onBlur={() => saveField('embedding_key', embeddingKey)} type="password" placeholder="Your API key" />
        <div className="flex gap-2 items-end">
          <div className="flex-1">
            <label className="block text-sm font-medium mb-1 text-muted-foreground">Model</label>
            <select
              value={selectedEmbeddingModel}
              onChange={(e) => { setSelectedEmbeddingModel(e.target.value); saveField('embedding_model', e.target.value); }}
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
          <button
            disabled={testingEmbedding || !embeddingUrl || !embeddingKey || !selectedEmbeddingModel}
            onClick={async () => {
              setTestingEmbedding(true);
              setEmbeddingTest(null);
              try {
                const res = await apiFetch<{ ok: boolean; message?: string; error?: string }>('/settings/test-embedding', {
                  method: 'POST',
                  body: JSON.stringify({ url: embeddingUrl, key: embeddingKey, model: selectedEmbeddingModel, dimension: embeddingDimension }),
                });
                setEmbeddingTest(res);
              } catch {
                setEmbeddingTest({ ok: false, error: 'Request failed' });
              } finally {
                setTestingEmbedding(false);
              }
            }}
            className="px-4 py-2 rounded-lg border border-input bg-card text-sm hover:bg-muted disabled:opacity-50"
          >
            {testingEmbedding ? 'Testing...' : 'Test'}
          </button>
        </div>
        <div>
          <label className="block text-sm font-medium mb-1 text-muted-foreground">Vector Dimension</label>
          <select
            value={embeddingDimension}
            onChange={(e) => { const v = Number(e.target.value); setEmbeddingDimension(v); saveField('embedding_dimension', v); }}
            className="w-full px-3 py-2 rounded-lg border border-input bg-card text-foreground text-sm focus:outline-none focus:ring-2 focus:ring-ring"
          >
            <option value={256}>256  —  ~100 MB / 100k photos</option>
            <option value={768}>768  —  ~300 MB / 100k photos (recommended)</option>
            <option value={1024}>1024  —  ~400 MB / 100k photos</option>
            <option value={3072}>3072  —  ~1.2 GB / 100k photos</option>
          </select>
          <p className="mt-1 text-xs text-muted-foreground">Lower dimensions save memory, higher dimensions improve search accuracy. 768 is a good balance.</p>
        </div>
        {embeddingTest && (
          <p className={`text-xs ${embeddingTest.ok ? 'text-green-600 dark:text-green-400' : 'text-destructive'}`}>
            {embeddingTest.ok ? embeddingTest.message : embeddingTest.error}
          </p>
        )}

        {/* Image-to-Text */}
        <div className="border-t border-border pt-4 mt-2">
          <div className="flex items-center justify-between">
            <div>
              <p className="text-sm font-medium">Image-to-Text Model</p>
              <p className="text-xs text-muted-foreground">If your embedding model doesn't support images, enable this to describe photos with a vision LLM before embedding.</p>
            </div>
            <label className="relative inline-flex items-center cursor-pointer">
              <input
                type="checkbox"
                checked={i2tEnabled}
                onChange={(e) => { setI2tEnabled(e.target.checked); saveField('i2t_enabled', e.target.checked); }}
                className="sr-only peer"
              />
              <div className="w-9 h-5 bg-muted rounded-full peer peer-checked:bg-primary after:content-[''] after:absolute after:top-0.5 after:left-[2px] after:bg-white after:rounded-full after:h-4 after:w-4 after:transition-all peer-checked:after:translate-x-full" />
            </label>
          </div>

          {i2tEnabled && (
            <div className="mt-4 space-y-4 pl-0">
              <div>
                <label className="block text-sm font-medium mb-1 text-muted-foreground">Provider</label>
                <select
                  value={i2tProvider}
                  onChange={(e) => { setI2tProvider(e.target.value); saveField('i2t_provider', e.target.value); }}
                  className="w-full px-3 py-2 rounded-lg border border-input bg-card text-foreground text-sm focus:outline-none focus:ring-2 focus:ring-ring"
                >
                  <option value="anthropic">Anthropic (/v1/messages)</option>
                  <option value="google">Google (/v1beta/generateContent)</option>
                  <option value="openai">OpenAI (/v1/chat/completions)</option>
                  <option value="openai_compat">OpenAI Compatible (/v1/responses)</option>
                </select>
              </div>
              <InputField label="API URL" value={i2tUrl} onChange={setI2tUrl} onBlur={() => saveField('i2t_url', i2tUrl)} placeholder="https://api.openai.com" />
              <InputField label="API Key" value={i2tKey} onChange={setI2tKey} onBlur={() => saveField('i2t_key', i2tKey)} type="password" placeholder="Your API key" />
              <div className="flex gap-2 items-end">
                <div className="flex-1">
                  <label className="block text-sm font-medium mb-1 text-muted-foreground">Model</label>
                  <select
                    value={selectedI2tModel}
                    onChange={(e) => { setSelectedI2tModel(e.target.value); saveField('i2t_model', e.target.value); }}
                    className="w-full px-3 py-2 rounded-lg border border-input bg-card text-foreground text-sm focus:outline-none focus:ring-2 focus:ring-ring"
                  >
                    <option value="">Select a model...</option>
                    {i2tModels.map((m) => (
                      <option key={m.id} value={m.id}>{m.name}</option>
                    ))}
                  </select>
                </div>
                <button
                  onClick={fetchI2tModels}
                  className="px-4 py-2 rounded-lg border border-input bg-card text-sm hover:bg-muted"
                >
                  Fetch Models
                </button>
                <button
                  disabled={testingI2t || !i2tUrl || !i2tKey || !selectedI2tModel}
                  onClick={async () => {
                    setTestingI2t(true);
                    setI2tTest(null);
                    try {
                      const res = await apiFetch<{ ok: boolean; message?: string; error?: string }>('/settings/test-i2t', {
                        method: 'POST',
                        body: JSON.stringify({ provider: i2tProvider, url: i2tUrl, key: i2tKey, model: selectedI2tModel }),
                      });
                      setI2tTest(res);
                    } catch {
                      setI2tTest({ ok: false, error: 'Request failed' });
                    } finally {
                      setTestingI2t(false);
                    }
                  }}
                  className="px-4 py-2 rounded-lg border border-input bg-card text-sm hover:bg-muted disabled:opacity-50"
                >
                  {testingI2t ? 'Testing...' : 'Test'}
                </button>
              </div>
              <div className="flex gap-2">
                <input
                  type="text"
                  value={manualI2tModel}
                  onChange={(e) => setManualI2tModel(e.target.value)}
                  placeholder="Or manually enter model ID..."
                  className="flex-1 px-3 py-2 rounded-lg border border-input bg-card text-foreground text-sm focus:outline-none focus:ring-2 focus:ring-ring"
                />
                <button
                  onClick={() => {
                    if (manualI2tModel.trim()) {
                      setI2tModels((prev) => [...prev, { id: manualI2tModel.trim(), name: manualI2tModel.trim() }]);
                      setSelectedI2tModel(manualI2tModel.trim());
                      saveField('i2t_model', manualI2tModel.trim());
                      setManualI2tModel('');
                    }
                  }}
                  className="px-3 py-2 rounded-lg bg-primary text-primary-foreground text-sm"
                >
                  Add
                </button>
              </div>
              {i2tTest && (
                <p className={`text-xs ${i2tTest.ok ? 'text-green-600 dark:text-green-400' : 'text-destructive'}`}>
                  {i2tTest.ok ? i2tTest.message : i2tTest.error}
                </p>
              )}
            </div>
          )}
        </div>
      </Section>

      {/* Agent Model */}
      <Section title="Agent Model">
        <div>
          <label className="block text-sm font-medium mb-1 text-muted-foreground">Provider</label>
          <select
            value={agentProvider}
            onChange={(e) => { setAgentProvider(e.target.value); saveField('agent_provider', e.target.value); }}
            className="w-full px-3 py-2 rounded-lg border border-input bg-card text-foreground text-sm focus:outline-none focus:ring-2 focus:ring-ring"
          >
            <option value="anthropic">Anthropic (/v1/messages)</option>
            <option value="google">Google (/v1beta/generateContent)</option>
            <option value="openai">OpenAI (/v1/chat/completions)</option>
            <option value="openai_compat">OpenAI Compatible (/v1/responses)</option>
          </select>
        </div>
        <InputField label="API URL" value={agentUrl} onChange={setAgentUrl} onBlur={() => saveField('agent_url', agentUrl)} placeholder="https://api.openai.com" hint={agentUrlHint} />
        <InputField label="API Key" value={agentKey} onChange={setAgentKey} onBlur={() => saveField('agent_key', agentKey)} type="password" placeholder="Your API key" />
        <div className="flex gap-2 items-end">
          <div className="flex-1">
            <label className="block text-sm font-medium mb-1 text-muted-foreground">Model</label>
            <select
              value={selectedAgentModel}
              onChange={(e) => { setSelectedAgentModel(e.target.value); saveField('agent_model', e.target.value); }}
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
          <button
            disabled={testingAgent || !agentUrl || !agentKey || !selectedAgentModel}
            onClick={async () => {
              setTestingAgent(true);
              setAgentTest(null);
              try {
                const res = await apiFetch<{ ok: boolean; message?: string; error?: string }>('/settings/test-agent', {
                  method: 'POST',
                  body: JSON.stringify({ provider: agentProvider, url: agentUrl, key: agentKey, model: selectedAgentModel }),
                });
                setAgentTest(res);
              } catch {
                setAgentTest({ ok: false, error: 'Request failed' });
              } finally {
                setTestingAgent(false);
              }
            }}
            className="px-4 py-2 rounded-lg border border-input bg-card text-sm hover:bg-muted disabled:opacity-50"
          >
            {testingAgent ? 'Testing...' : 'Test'}
          </button>
        </div>
        <div className="flex gap-2">
          <input
            type="text"
            value={manualModelName}
            onChange={(e) => setManualModelName(e.target.value)}
            placeholder="Or manually enter model ID..."
            className="flex-1 px-3 py-2 rounded-lg border border-input bg-card text-foreground text-sm focus:outline-none focus:ring-2 focus:ring-ring"
          />
          <button
            onClick={() => {
              if (manualModelName.trim()) {
                setAgentModels((prev) => [...prev, { id: manualModelName.trim(), name: manualModelName.trim() }]);
                setSelectedAgentModel(manualModelName.trim());
                saveField('agent_model', manualModelName.trim());
                setManualModelName('');
              }
            }}
            className="px-3 py-2 rounded-lg bg-primary text-primary-foreground text-sm"
          >
            Add
          </button>
        </div>
        {agentTest && (
          <p className={`text-xs ${agentTest.ok ? 'text-green-600 dark:text-green-400' : 'text-destructive'}`}>
            {agentTest.ok ? agentTest.message : agentTest.error}
          </p>
        )}
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
                  {isAutoApproved(tool.id) && (
                    <span className="inline-flex items-center gap-1 px-1.5 py-0.5 rounded text-xs font-medium bg-green-500/10 text-green-600 dark:text-green-400">
                      <ShieldCheck className="w-3 h-3" /> Auto
                    </span>
                  )}
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
                  <div className="flex items-center justify-between">
                    <div className="flex items-center gap-2">
                      <ShieldCheck className="w-4 h-4" />
                      <span className="text-sm">Auto-Approve in Chat</span>
                    </div>
                    <label className="relative inline-flex items-center cursor-pointer" onClick={(e) => e.stopPropagation()}>
                      <input
                        type="checkbox"
                        checked={isAutoApproved(tool.id)}
                        onChange={() => toggleAutoApprove(tool.id)}
                        className="sr-only peer"
                      />
                      <div className="w-9 h-5 bg-muted rounded-full peer peer-checked:bg-green-500 after:content-[''] after:absolute after:top-0.5 after:left-[2px] after:bg-white after:rounded-full after:h-4 after:w-4 after:transition-all peer-checked:after:translate-x-full" />
                    </label>
                  </div>
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

      {/* Scan & Embed */}
      <Section title="Scan & Embed">
        {/* Concurrency setting */}
        <div className="flex items-center justify-between">
          <div>
            <p className="text-sm font-medium">Concurrency</p>
            <p className="text-xs text-muted-foreground">Number of photos to embed in parallel</p>
          </div>
          <select
            value={embeddingConcurrency}
            onChange={(e) => { const v = Number(e.target.value); setEmbeddingConcurrency(v); saveField('embedding_concurrency', v); }}
            disabled={!!isRunning}
            className="px-3 py-1.5 rounded-lg border border-input bg-card text-foreground text-sm focus:outline-none focus:ring-2 focus:ring-ring disabled:opacity-50"
          >
            {[1, 2, 4, 8, 16].map((n) => (
              <option key={n} value={n}>{n}</option>
            ))}
          </select>
        </div>

        {isRunning || (scanProgress && scanProgress.phase === 'done') || (scanProgress && scanProgress.phase === 'error') ? (
          <div className="space-y-3">
            {/* Phase label */}
            <div className="flex items-center justify-between">
              <span className={`inline-block px-2 py-0.5 rounded text-xs font-medium ${
                scanProgress!.phase === 'scanning' ? 'bg-blue-500/10 text-blue-600 dark:text-blue-400' :
                scanProgress!.phase === 'embedding' ? 'bg-primary/10 text-primary' :
                scanProgress!.phase === 'paused' ? 'bg-yellow-500/10 text-yellow-600 dark:text-yellow-400' :
                scanProgress!.phase === 'pausing' ? 'bg-yellow-500/10 text-yellow-600 dark:text-yellow-400' :
                scanProgress!.phase === 'stopping' ? 'bg-red-500/10 text-destructive' :
                scanProgress!.phase === 'done' ? 'bg-green-500/10 text-green-600 dark:text-green-400' :
                scanProgress!.phase === 'error' ? 'bg-red-500/10 text-destructive' :
                'bg-muted text-muted-foreground'
              }`}>
                {scanProgress!.phase === 'scanning' ? 'Scanning...' :
                 scanProgress!.phase === 'embedding' ? 'Embedding...' :
                 scanProgress!.phase === 'paused' ? 'Paused' :
                 scanProgress!.phase === 'pausing' ? 'Pausing...' :
                 scanProgress!.phase === 'stopping' ? 'Stopping...' :
                 scanProgress!.phase === 'done' ? 'Completed' :
                 scanProgress!.phase === 'error' ? 'Error' :
                 scanProgress!.phase}
              </span>
              {scanProgress!.started_at && (
                <span className="text-xs text-muted-foreground">
                  Started {new Date(scanProgress!.started_at).toLocaleTimeString()}
                </span>
              )}
            </div>

            {/* Progress bar */}
            {scanProgress!.total > 0 && (
              <div>
                <div className="flex justify-between text-xs text-muted-foreground mb-1">
                  <span>{scanProgress!.processed} / {scanProgress!.total} photos</span>
                  <span>{scanProgress!.total > 0 ? Math.round((scanProgress!.processed / scanProgress!.total) * 100) : 0}%</span>
                </div>
                <div className="w-full bg-muted rounded-full h-2 overflow-hidden">
                  <div
                    className={`h-full rounded-full transition-all duration-300 ${
                      scanProgress!.phase === 'paused' ? 'bg-yellow-500' :
                      scanProgress!.phase === 'error' ? 'bg-destructive' :
                      scanProgress!.phase === 'done' ? 'bg-green-500' :
                      'bg-primary'
                    }`}
                    style={{ width: `${scanProgress!.total > 0 ? (scanProgress!.processed / scanProgress!.total) * 100 : 0}%` }}
                  />
                </div>
              </div>
            )}

            {/* Stats */}
            {(() => {
              const hasVision = scanProgress!.vision_calls > 0;
              const formatTokens = (n: number) => n > 1000 ? `${(n / 1000).toFixed(1)}k` : String(n);
              return (
                <div className={`grid gap-3 text-sm ${hasVision ? 'grid-cols-4' : 'grid-cols-2'}`}>
                  {hasVision && (
                    <div className="bg-muted rounded-lg p-2 text-center">
                      <p className="text-xs text-muted-foreground">Vision</p>
                      <p className="font-semibold">{formatTokens(scanProgress!.vision_tokens)}</p>
                      <p className="text-xs text-muted-foreground">{scanProgress!.vision_calls} calls</p>
                    </div>
                  )}
                  <div className="bg-muted rounded-lg p-2 text-center">
                    <p className="text-xs text-muted-foreground">Embed</p>
                    <p className="font-semibold">{formatTokens(scanProgress!.embed_tokens)}</p>
                    <p className="text-xs text-muted-foreground">{scanProgress!.embed_calls} calls</p>
                  </div>
                  {hasVision && (
                    <div className="bg-muted rounded-lg p-2 text-center">
                      <p className="text-xs text-muted-foreground">Total Tokens</p>
                      <p className="font-semibold">{formatTokens(scanProgress!.vision_tokens + scanProgress!.embed_tokens)}</p>
                    </div>
                  )}
                  <div className="bg-muted rounded-lg p-2 text-center">
                    <p className="text-xs text-muted-foreground">Failed</p>
                    <p className={`font-semibold ${scanProgress!.failed > 0 ? 'text-destructive' : ''}`}>
                      {scanProgress!.failed}
                    </p>
                  </div>
                </div>
              );
            })()}

            {/* Current file */}
            {scanProgress!.current_file && (
              <p className="text-xs text-muted-foreground truncate">
                Processing: {scanProgress!.current_file}
              </p>
            )}

            {/* Error message */}
            {scanProgress!.error && (
              <p className="text-xs text-destructive">{scanProgress!.error}</p>
            )}

            {/* Hint */}
            {scanProgress!.phase === 'paused' && (
              <p className="text-xs text-muted-foreground">If you need to change models, please stop the task first and restart after switching.</p>
            )}

            {/* Control buttons */}
            <div className="flex gap-2">
              {isRunning && !['paused', 'pausing', 'stopping'].includes(scanProgress!.phase) && (
                <button
                  onClick={() => apiFetch('/scan/pause', { method: 'POST' })}
                  className="flex-1 flex items-center justify-center gap-2 px-4 py-2 rounded-lg border border-input bg-card text-sm font-medium hover:bg-muted"
                >
                  <Pause className="w-4 h-4" />
                  Pause
                </button>
              )}
              {scanProgress!.phase === 'pausing' && (
                <button
                  disabled
                  className="flex-1 flex items-center justify-center gap-2 px-4 py-2 rounded-lg border border-input bg-card text-sm font-medium opacity-50 cursor-not-allowed"
                >
                  <Pause className="w-4 h-4" />
                  Pausing...
                </button>
              )}
              {scanProgress!.phase === 'paused' && (
                <button
                  onClick={() => apiFetch('/scan/resume', { method: 'POST' })}
                  className="flex-1 flex items-center justify-center gap-2 px-4 py-2 rounded-lg bg-primary text-primary-foreground text-sm font-medium hover:opacity-90"
                >
                  <Play className="w-4 h-4" />
                  Resume
                </button>
              )}
              {isRunning && scanProgress!.phase !== 'stopping' && (
                <button
                  onClick={() => apiFetch('/scan/stop', { method: 'POST' })}
                  className="flex items-center justify-center gap-2 px-4 py-2 rounded-lg border border-destructive/50 text-destructive text-sm font-medium hover:bg-destructive/10"
                >
                  <Square className="w-4 h-4" />
                  Stop
                </button>
              )}
              {scanProgress!.phase === 'stopping' && (
                <button
                  disabled
                  className="flex items-center justify-center gap-2 px-4 py-2 rounded-lg border border-destructive/50 text-destructive text-sm font-medium opacity-50 cursor-not-allowed"
                >
                  <Square className="w-4 h-4" />
                  Stopping...
                </button>
              )}
              {!isRunning && (
                <button
                  onClick={async () => {
                    await apiFetch('/scan', { method: 'POST' });
                    startPolling();
                  }}
                  className="flex-1 flex items-center justify-center gap-2 px-4 py-2 rounded-lg bg-primary text-primary-foreground text-sm font-medium hover:opacity-90"
                >
                  <RefreshCw className="w-4 h-4" />
                  Scan Again
                </button>
              )}
            </div>
          </div>
        ) : (
          <button
            onClick={async () => {
              await apiFetch('/scan', { method: 'POST' });
              startPolling();
            }}
            className="w-full flex items-center justify-center gap-2 px-6 py-3 rounded-xl bg-primary text-primary-foreground font-medium hover:opacity-90"
          >
            <RefreshCw className="w-5 h-5" />
            Scan & Embed
          </button>
        )}
      </Section>

      <AddToolDialog
        open={showAddTool}
        onClose={() => setShowAddTool(false)}
        onAdded={loadSettings}
      />
      <DirBrowserDialog
        open={showDirBrowser}
        onClose={() => setShowDirBrowser(false)}
        onSelect={async (path) => {
          if (!scanDirs.includes(path)) {
            const updated = [...scanDirs, path];
            setScanDirs(updated);
            saveScanDirs(updated);
          }
        }}
      />
    </div>
  );
}
