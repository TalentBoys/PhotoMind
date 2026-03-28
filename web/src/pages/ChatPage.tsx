import { useState, useRef, useEffect, useCallback, type FormEvent } from 'react';
import { Send, ImagePlus, CheckCircle, XCircle, Plus, Trash2, MessageSquare, ShieldCheck, Loader2, X } from 'lucide-react';
import PhotoLightbox, { type PhotoItem } from '@/components/PhotoLightbox';

interface Message {
  id: string;
  role: 'user' | 'assistant' | 'tool_confirm' | 'tool_result';
  content: string;
  tool_call?: ToolCall;
  search_results?: SearchResultItem[];
  imageUrl?: string;
}

interface ToolCall {
  execution_id: number;
  tool_name: string;
  params: Record<string, unknown>;
}

interface SearchResultItem {
  id: number;
  file_name: string;
  file_path: string;
  score: number;
}

interface Session {
  session_id: string;
  title: string;
  last_message_at: string;
}

interface ApiChatResponse {
  content: string;
  tool_calls?: ToolCall[];
  auto_results?: { tool_name: string; params: Record<string, unknown>; result: unknown }[];
  done: boolean;
}

/** Extract displayable photo items from a tool result value (search_photos array or get_photo_info single object) */
function extractPhotos(result: Record<string, unknown>): SearchResultItem[] | null {
  // search_photos: { results: [...], count: N }
  if (result?.results && Array.isArray(result.results) && result.results.length > 0) {
    return result.results as SearchResultItem[];
  }
  // get_photo_info: { id, file_name, file_path, ... }
  if (typeof result?.id === 'number' && typeof result?.file_name === 'string') {
    return [{
      id: result.id as number,
      file_name: result.file_name as string,
      file_path: (result.file_path as string) || '',
      score: 1,
    }];
  }
  return null;
}

const LS_KEY = 'photomind_active_session';

export default function ChatPage() {
  const [sessions, setSessions] = useState<Session[]>([]);
  const [activeSessionId, setActiveSessionId] = useState<string>(
    () => localStorage.getItem(LS_KEY) || crypto.randomUUID()
  );
  const [messages, setMessages] = useState<Message[]>([]);
  const [input, setInput] = useState('');
  const [loading, setLoading] = useState(false);
  const [lightbox, setLightbox] = useState<{ photos: PhotoItem[]; index: number } | null>(null);
  const [attachedImage, setAttachedImage] = useState<{ file: File; previewUrl: string } | null>(null);
  const [autoApproveTools, setAutoApproveTools] = useState<Set<string>>(() => {
    try {
      const saved = localStorage.getItem('photomind_auto_approve_tools');
      return saved ? new Set(JSON.parse(saved)) : new Set();
    } catch { return new Set(); }
  });
  const bottomRef = useRef<HTMLDivElement>(null);

  useEffect(() => { localStorage.setItem(LS_KEY, activeSessionId); }, [activeSessionId]);

  const loadSessions = useCallback(async () => {
    try {
      const res = await fetch('/api/chat/sessions');
      if (res.ok) setSessions(await res.json());
    } catch { /* ignore */ }
  }, []);

  useEffect(() => { loadSessions(); }, [loadSessions]);

  const loadMessages = useCallback(async (sessionId: string) => {
    try {
      const res = await fetch(`/api/chat/sessions/${sessionId}/messages`);
      if (res.ok) {
        const data = await res.json();
        const msgs: Message[] = [];
        for (const m of data) {
          if (m.role === 'user' || m.role === 'assistant') {
            const imageUrl = m.role === 'user' && m.metadata?.image_filename
              ? `/api/chat/images/${m.metadata.image_filename}`
              : undefined;
            msgs.push({ id: String(m.id), role: m.role, content: m.content, imageUrl });
          }
          // Tool result messages are part of the loop context, show them as photo results
          if (m.role === 'tool') {
            try {
              const resultVal = JSON.parse(m.content);
              const photos = extractPhotos(resultVal);
              if (photos) {
                msgs.push({
                  id: String(m.id),
                  role: 'tool_result',
                  content: photos.length === 1 ? photos[0].file_name : `Found ${photos.length} photo${photos.length > 1 ? 's' : ''}`,
                  search_results: photos,
                });
              }
            } catch { /* not a photo result, skip */ }
          }
        }
        setMessages(msgs);
      } else {
        setMessages([]);
      }
    } catch { setMessages([]); }
  }, []);

  useEffect(() => { loadMessages(activeSessionId); }, [activeSessionId, loadMessages]);

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, [messages]);

  const switchSession = (sessionId: string) => {
    if (sessionId === activeSessionId) return;
    setActiveSessionId(sessionId);
  };

  const createNewSession = () => {
    setActiveSessionId(crypto.randomUUID());
    setMessages([]);
  };

  const deleteSession = async (sessionId: string) => {
    try {
      await fetch(`/api/chat/sessions/${sessionId}`, { method: 'DELETE' });
      setSessions((prev) => prev.filter((s) => s.session_id !== sessionId));
      if (sessionId === activeSessionId) createNewSession();
    } catch { /* ignore */ }
  };

  /** Process a chat API response: add messages, handle auto_results, return pending tool_calls */
  const processChatResponse = (data: ApiChatResponse): ToolCall[] => {
    // Add assistant content if any
    if (data.content) {
      setMessages((prev) => [...prev, {
        id: crypto.randomUUID(), role: 'assistant', content: data.content,
      }]);
    }

    // Show auto-executed tool results (especially search results with photos)
    if (data.auto_results) {
      for (const ar of data.auto_results) {
        const result = ar.result as Record<string, unknown>;
        const photos = extractPhotos(result);
        if (photos) {
          setMessages((prev) => [...prev, {
            id: crypto.randomUUID(),
            role: 'tool_result',
            content: photos.length === 1 ? photos[0].file_name : `Found ${photos.length} photo${photos.length > 1 ? 's' : ''}`,
            search_results: photos,
          }]);
        }
      }
    }

    return data.tool_calls ?? [];
  };

  const clearAttachedImage = () => {
    if (attachedImage) {
      URL.revokeObjectURL(attachedImage.previewUrl);
      setAttachedImage(null);
    }
  };

  const handleImageSelect = (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    if (file) {
      clearAttachedImage();
      setAttachedImage({ file, previewUrl: URL.createObjectURL(file) });
    }
    e.target.value = '';
  };

  const sendMessage = async (e: FormEvent) => {
    e.preventDefault();
    if ((!input.trim() && !attachedImage) || loading) return;
    const userMsg: Message = {
      id: crypto.randomUUID(),
      role: 'user',
      content: input.trim(),
      imageUrl: attachedImage?.previewUrl,
    };
    setMessages((prev) => [...prev, userMsg]);
    const currentImage = attachedImage;
    setInput('');
    setAttachedImage(null);
    setLoading(true);

    try {
      let res: Response;
      if (currentImage) {
        const formData = new FormData();
        formData.append('session_id', activeSessionId);
        formData.append('message', userMsg.content);
        formData.append('auto_approve_tools', JSON.stringify([...autoApproveTools]));
        formData.append('image', currentImage.file);
        res = await fetch('/api/chat/upload', { method: 'POST', body: formData });
      } else {
        res = await fetch('/api/chat', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({
            session_id: activeSessionId,
            message: userMsg.content,
            auto_approve_tools: [...autoApproveTools],
          }),
        });
      }
      if (res.ok) {
        const data: ApiChatResponse = await res.json();
        const pendingTools = processChatResponse(data);

        // Show pending tool confirmations
        for (const tc of pendingTools) {
          setMessages((prev) => [...prev, {
            id: crypto.randomUUID(),
            role: 'tool_confirm',
            content: `Tool: ${tc.tool_name}`,
            tool_call: tc,
          }]);
        }
      }
      loadSessions();
    } finally {
      setLoading(false);
    }
  };

  /** Confirm or cancel pending tools, then continue the agent loop */
  const confirmTools = async (toolCalls: ToolCall[], confirmed: boolean) => {
    setLoading(true);

    // Mark UI messages
    const execIds = new Set(toolCalls.map(tc => tc.execution_id));
    setMessages((prev) => prev.map((m) =>
      m.tool_call && execIds.has(m.tool_call.execution_id) && !m.content.includes('—')
        ? { ...m, content: `${m.content} — ${confirmed ? 'Confirmed' : 'Cancelled'}` }
        : m
    ));

    try {
      const res = await fetch('/api/chat/continue', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          session_id: activeSessionId,
          tool_results: toolCalls.map(tc => ({
            execution_id: tc.execution_id,
            confirmed,
          })),
          auto_approve_tools: [...autoApproveTools],
        }),
      });

      if (res.ok) {
        const data: ApiChatResponse = await res.json();
        const pendingTools = processChatResponse(data);

        for (const tc of pendingTools) {
          setMessages((prev) => [...prev, {
            id: crypto.randomUUID(),
            role: 'tool_confirm',
            content: `Tool: ${tc.tool_name}`,
            tool_call: tc,
          }]);
        }
      }
    } finally {
      setLoading(false);
    }
  };

  const confirmToolAlways = (tc: ToolCall) => {
    const next = new Set(autoApproveTools);
    next.add(tc.tool_name);
    setAutoApproveTools(next);
    localStorage.setItem('photomind_auto_approve_tools', JSON.stringify([...next]));
    confirmTools([tc], true);
  };

  const isNewSession = !sessions.some((s) => s.session_id === activeSessionId);

  return (
    <div className="flex h-[calc(100vh-3.5rem)]">
      {/* Left sidebar */}
      <div className="w-64 border-r border-border flex flex-col bg-card shrink-0">
        <div className="p-3">
          <button
            onClick={createNewSession}
            className="w-full flex items-center gap-2 px-3 py-2 rounded-lg border border-border text-sm font-medium hover:bg-muted transition-colors"
          >
            <Plus className="w-4 h-4" /> New Chat
          </button>
        </div>
        <div className="flex-1 overflow-y-auto px-2 pb-2 space-y-0.5">
          {isNewSession && (
            <div className="flex items-center gap-2 px-3 py-2.5 rounded-lg bg-primary/10 text-sm">
              <MessageSquare className="w-4 h-4 shrink-0 text-primary" />
              <span className="truncate font-medium text-primary">New Chat</span>
            </div>
          )}
          {sessions.map((s) => (
            <div
              key={s.session_id}
              onClick={() => switchSession(s.session_id)}
              className={`group flex items-center gap-2 px-3 py-2.5 rounded-lg text-sm cursor-pointer transition-colors ${
                s.session_id === activeSessionId
                  ? 'bg-primary/10 text-foreground'
                  : 'text-muted-foreground hover:bg-muted'
              }`}
            >
              <MessageSquare className="w-4 h-4 shrink-0" />
              <span className="truncate flex-1">{s.title}</span>
              <button
                onClick={(e) => { e.stopPropagation(); deleteSession(s.session_id); }}
                className="opacity-0 group-hover:opacity-100 p-0.5 rounded hover:bg-destructive/10 hover:text-destructive transition-opacity"
              >
                <Trash2 className="w-3.5 h-3.5" />
              </button>
            </div>
          ))}
        </div>
      </div>

      {/* Right main area */}
      <div className="flex-1 flex flex-col min-w-0">
        <div className="flex-1 overflow-y-auto px-4 py-6 space-y-4">
          {messages.length === 0 && (
            <div className="flex flex-col items-center justify-center h-full text-muted-foreground">
              <p className="text-lg">Chat with PhotoMind</p>
              <p className="text-sm">Ask me to find photos, move files, or manage your library.</p>
            </div>
          )}
          {messages.map((m) => (
            <div
              key={m.id}
              className={`max-w-2xl mx-auto flex ${m.role === 'user' ? 'justify-end' : 'justify-start'}`}
            >
              {m.role === 'tool_result' && m.search_results ? (
                <div className="w-full space-y-2">
                  <p className="text-sm text-muted-foreground">{m.content}</p>
                  <div className="grid grid-cols-3 sm:grid-cols-4 md:grid-cols-5 gap-2">
                    {m.search_results.map((r, idx) => (
                      <div key={r.id} className="group relative aspect-square rounded-lg overflow-hidden border border-border bg-muted cursor-pointer" onClick={() => setLightbox({ photos: m.search_results!.map(p => ({ id: p.id, file_name: p.file_name })), index: idx })}>
                        <img src={`/api/photos/${r.id}/thumbnail`} alt={r.file_name} className="w-full h-full object-cover" loading="lazy" />
                        <div className="absolute inset-0 bg-black/60 opacity-0 group-hover:opacity-100 transition-opacity flex flex-col justify-end p-1.5 text-white text-xs">
                          <p className="truncate">{r.file_name}</p>
                          <p className="opacity-75">{(r.score * 100).toFixed(1)}%</p>
                        </div>
                      </div>
                    ))}
                  </div>
                </div>
              ) : m.role === 'tool_confirm' ? (
                <div className="bg-accent border border-border rounded-xl p-4 space-y-2">
                  <p className="font-medium">{m.tool_call?.tool_name}</p>
                  <pre className="text-xs bg-muted p-2 rounded overflow-x-auto">
                    {JSON.stringify(m.tool_call?.params, null, 2)}
                  </pre>
                  {!m.content.includes('—') && m.tool_call && (
                    <div className="flex gap-2">
                      <button
                        onClick={() => confirmTools([m.tool_call!], true)}
                        className="flex items-center gap-1 px-3 py-1.5 rounded-lg bg-green-600 text-white text-sm hover:opacity-90"
                      >
                        <CheckCircle className="w-4 h-4" /> Confirm
                      </button>
                      <button
                        onClick={() => confirmToolAlways(m.tool_call!)}
                        className="flex items-center gap-1 px-3 py-1.5 rounded-lg bg-primary text-primary-foreground text-sm hover:opacity-90"
                      >
                        <ShieldCheck className="w-4 h-4" /> Always Allow
                      </button>
                      <button
                        onClick={() => confirmTools([m.tool_call!], false)}
                        className="flex items-center gap-1 px-3 py-1.5 rounded-lg bg-destructive text-destructive-foreground text-sm hover:opacity-90"
                      >
                        <XCircle className="w-4 h-4" /> Cancel
                      </button>
                    </div>
                  )}
                </div>
              ) : (
                <div className={`rounded-xl px-4 py-2.5 ${
                  m.role === 'user' ? 'bg-primary text-primary-foreground' : 'bg-card border border-border'
                }`}>
                  {m.imageUrl && (
                    <img src={m.imageUrl} alt="Attached" className="max-w-48 max-h-48 rounded-lg mb-2 object-cover" />
                  )}
                  {m.content && <p className="whitespace-pre-wrap">{m.content}</p>}
                </div>
              )}
            </div>
          ))}
          {loading && (
            <div className="max-w-2xl mx-auto flex justify-start">
              <div className="bg-card border border-border rounded-xl px-4 py-2.5 text-muted-foreground flex items-center gap-2">
                <Loader2 className="w-4 h-4 animate-spin" /> Thinking...
              </div>
            </div>
          )}
          <div ref={bottomRef} />
        </div>

        <div className="border-t border-border max-w-2xl mx-auto w-full">
          {attachedImage && (
            <div className="px-4 pt-3 flex items-center gap-2">
              <img src={attachedImage.previewUrl} alt="Attached" className="w-12 h-12 rounded-lg object-cover border border-border" />
              <span className="text-sm text-muted-foreground truncate flex-1">{attachedImage.file.name}</span>
              <button type="button" onClick={clearAttachedImage} className="p-1 rounded-lg hover:bg-muted">
                <X className="w-4 h-4 text-muted-foreground" />
              </button>
            </div>
          )}
          <form onSubmit={sendMessage} className="p-4 flex gap-2">
            <label className="px-3 py-2.5 rounded-xl border border-input bg-card hover:bg-muted cursor-pointer flex items-center">
              <ImagePlus className="w-5 h-5 text-muted-foreground" />
              <input type="file" accept="image/*" className="hidden" onChange={handleImageSelect} />
            </label>
          <input
            type="text"
            value={input}
            onChange={(e) => setInput(e.target.value)}
            placeholder="Ask about your photos..."
            className="flex-1 px-4 py-2.5 rounded-xl border border-input bg-card text-foreground focus:outline-none focus:ring-2 focus:ring-ring"
          />
          <button
            type="submit"
            disabled={loading || (!input.trim() && !attachedImage)}
            className="px-4 py-2.5 rounded-xl bg-primary text-primary-foreground hover:opacity-90 disabled:opacity-50"
          >
            <Send className="w-5 h-5" />
          </button>
        </form>
        </div>
      </div>

      <PhotoLightbox
        photos={lightbox?.photos ?? []}
        openIndex={lightbox?.index ?? null}
        onClose={() => setLightbox(null)}
      />
    </div>
  );
}
