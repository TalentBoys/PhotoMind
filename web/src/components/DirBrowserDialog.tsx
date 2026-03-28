import { useState, useEffect, useCallback } from 'react';
import { X, Folder, ChevronRight, ArrowUp, Loader2 } from 'lucide-react';
import { apiFetch } from '@/lib/api';

interface DirEntry {
  name: string;
  path: string;
}

interface BrowseResponse {
  current: string;
  parent: string | null;
  dirs: DirEntry[];
}

interface DirBrowserDialogProps {
  open: boolean;
  onClose: () => void;
  onSelect: (path: string) => void;
}

export default function DirBrowserDialog({ open, onClose, onSelect }: DirBrowserDialogProps) {
  const [current, setCurrent] = useState('/');
  const [parent, setParent] = useState<string | null>(null);
  const [dirs, setDirs] = useState<DirEntry[]>([]);
  const [loading, setLoading] = useState(false);
  const [selecting, setSelecting] = useState(false);
  const [error, setError] = useState('');

  const browse = useCallback(async (path: string) => {
    setLoading(true);
    setError('');
    try {
      const res = await apiFetch<BrowseResponse>('/browse-dirs', {
        method: 'POST',
        body: JSON.stringify({ path }),
      });
      setCurrent(res.current);
      setParent(res.parent);
      setDirs(res.dirs);
    } catch {
      setError('Cannot access this directory');
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    if (open) {
      browse('/');
    }
  }, [open, browse]);

  if (!open) return null;

  const pathParts = current.split('/').filter(Boolean);

  return (
    <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50 p-4">
      <div className="bg-card border border-border rounded-xl w-full max-w-lg max-h-[70vh] flex flex-col">
        {/* Header */}
        <div className="flex items-center justify-between px-4 py-3 border-b border-border">
          <h3 className="font-semibold">Select Directory</h3>
          <button onClick={onClose} className="p-1 hover:opacity-75">
            <X className="w-5 h-5" />
          </button>
        </div>

        {/* Breadcrumb */}
        <div className="flex items-center gap-1 px-4 py-2 border-b border-border text-sm overflow-x-auto">
          <button
            onClick={() => browse('/')}
            className="text-primary hover:underline shrink-0"
          >
            /
          </button>
          {pathParts.map((part, i) => {
            const fullPath = '/' + pathParts.slice(0, i + 1).join('/');
            const isLast = i === pathParts.length - 1;
            return (
              <span key={fullPath} className="flex items-center gap-1 shrink-0">
                <ChevronRight className="w-3 h-3 text-muted-foreground" />
                {isLast ? (
                  <span className="font-medium">{part}</span>
                ) : (
                  <button
                    onClick={() => browse(fullPath)}
                    className="text-primary hover:underline"
                  >
                    {part}
                  </button>
                )}
              </span>
            );
          })}
        </div>

        {/* Directory list */}
        <div className="flex-1 overflow-y-auto min-h-0">
          {loading ? (
            <div className="flex items-center justify-center py-12">
              <Loader2 className="w-5 h-5 animate-spin text-muted-foreground" />
            </div>
          ) : error ? (
            <div className="px-4 py-8 text-center text-sm text-destructive">{error}</div>
          ) : (
            <div className="divide-y divide-border">
              {parent && (
                <button
                  onClick={() => browse(parent)}
                  className="flex items-center gap-3 w-full px-4 py-2.5 hover:bg-muted text-left text-sm"
                >
                  <ArrowUp className="w-4 h-4 text-muted-foreground" />
                  <span className="text-muted-foreground">..</span>
                </button>
              )}
              {dirs.length === 0 && !parent && (
                <div className="px-4 py-8 text-center text-sm text-muted-foreground">
                  No subdirectories
                </div>
              )}
              {dirs.map((dir) => (
                <button
                  key={dir.path}
                  onClick={() => browse(dir.path)}
                  className="flex items-center gap-3 w-full px-4 py-2.5 hover:bg-muted text-left text-sm"
                >
                  <Folder className="w-4 h-4 text-primary" />
                  <span>{dir.name}</span>
                </button>
              ))}
            </div>
          )}
        </div>

        {/* Footer */}
        <div className="flex items-center justify-between px-4 py-3 border-t border-border">
          <span className="text-xs text-muted-foreground truncate mr-4" title={current}>
            {current}
          </span>
          <div className="flex gap-2 shrink-0">
            <button
              onClick={onClose}
              className="px-4 py-2 rounded-lg border border-input bg-card text-sm hover:bg-muted"
            >
              Cancel
            </button>
            <button
              disabled={selecting}
              onClick={async () => {
                setSelecting(true);
                try {
                  await onSelect(current);
                } finally {
                  setSelecting(false);
                  onClose();
                }
              }}
              className="px-4 py-2 rounded-lg bg-primary text-primary-foreground text-sm disabled:opacity-50"
            >
              {selecting ? 'Adding...' : 'Select'}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
