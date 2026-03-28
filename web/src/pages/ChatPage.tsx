import { useState, useRef, useEffect, type FormEvent } from 'react';
import { Send, ImagePlus, CheckCircle, XCircle } from 'lucide-react';

interface Message {
  id: string;
  role: 'user' | 'assistant' | 'tool_confirm';
  content: string;
  images?: string[];
  tool_call?: ToolCall;
}

interface ToolCall {
  execution_id: number;
  tool_name: string;
  params: Record<string, unknown>;
}

export default function ChatPage() {
  const [messages, setMessages] = useState<Message[]>([]);
  const [input, setInput] = useState('');
  const [loading, setLoading] = useState(false);
  const [sessionId] = useState(() => crypto.randomUUID());
  const bottomRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, [messages]);

  const sendMessage = async (e: FormEvent) => {
    e.preventDefault();
    if (!input.trim() || loading) return;
    const userMsg: Message = { id: crypto.randomUUID(), role: 'user', content: input.trim() };
    setMessages((prev) => [...prev, userMsg]);
    setInput('');
    setLoading(true);

    try {
      const res = await fetch('/api/chat', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ session_id: sessionId, message: userMsg.content }),
      });
      if (res.ok) {
        const data = await res.json();
        const assistantMsg: Message = {
          id: crypto.randomUUID(),
          role: 'assistant',
          content: data.content ?? '',
        };
        setMessages((prev) => [...prev, assistantMsg]);

        if (data.tool_calls) {
          for (const tc of data.tool_calls) {
            setMessages((prev) => [
              ...prev,
              {
                id: crypto.randomUUID(),
                role: 'tool_confirm',
                content: `Tool: ${tc.tool_name}`,
                tool_call: tc,
              },
            ]);
          }
        }
      }
    } finally {
      setLoading(false);
    }
  };

  const confirmTool = async (executionId: number, confirmed: boolean) => {
    await fetch('/api/chat/confirm-tool', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ execution_id: executionId, confirmed }),
    });
    setMessages((prev) =>
      prev.map((m) =>
        m.tool_call?.execution_id === executionId
          ? { ...m, content: `${m.content} — ${confirmed ? 'Confirmed' : 'Cancelled'}` }
          : m
      )
    );
  };

  return (
    <div className="flex flex-col h-[calc(100vh-3.5rem)]">
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
            {m.role === 'tool_confirm' ? (
              <div className="bg-accent border border-border rounded-xl p-4 space-y-2">
                <p className="font-medium">{m.tool_call?.tool_name}</p>
                <pre className="text-xs bg-muted p-2 rounded overflow-x-auto">
                  {JSON.stringify(m.tool_call?.params, null, 2)}
                </pre>
                {!m.content.includes('—') && (
                  <div className="flex gap-2">
                    <button
                      onClick={() => confirmTool(m.tool_call!.execution_id, true)}
                      className="flex items-center gap-1 px-3 py-1.5 rounded-lg bg-green-600 text-white text-sm hover:opacity-90"
                    >
                      <CheckCircle className="w-4 h-4" /> Confirm
                    </button>
                    <button
                      onClick={() => confirmTool(m.tool_call!.execution_id, false)}
                      className="flex items-center gap-1 px-3 py-1.5 rounded-lg bg-destructive text-destructive-foreground text-sm hover:opacity-90"
                    >
                      <XCircle className="w-4 h-4" /> Cancel
                    </button>
                  </div>
                )}
              </div>
            ) : (
              <div
                className={`rounded-xl px-4 py-2.5 ${
                  m.role === 'user'
                    ? 'bg-primary text-primary-foreground'
                    : 'bg-card border border-border'
                }`}
              >
                <p className="whitespace-pre-wrap">{m.content}</p>
              </div>
            )}
          </div>
        ))}
        {loading && (
          <div className="max-w-2xl mx-auto flex justify-start">
            <div className="bg-card border border-border rounded-xl px-4 py-2.5 text-muted-foreground">
              Thinking...
            </div>
          </div>
        )}
        <div ref={bottomRef} />
      </div>

      <form onSubmit={sendMessage} className="border-t border-border p-4 flex gap-2 max-w-2xl mx-auto w-full">
        <label className="px-3 py-2.5 rounded-xl border border-input bg-card hover:bg-muted cursor-pointer flex items-center">
          <ImagePlus className="w-5 h-5 text-muted-foreground" />
          <input type="file" accept="image/*" className="hidden" onChange={() => {}} />
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
          disabled={loading || !input.trim()}
          className="px-4 py-2.5 rounded-xl bg-primary text-primary-foreground hover:opacity-90 disabled:opacity-50"
        >
          <Send className="w-5 h-5" />
        </button>
      </form>
    </div>
  );
}
