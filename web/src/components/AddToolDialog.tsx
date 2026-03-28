import { useState } from 'react';
import { X } from 'lucide-react';
import { apiFetch } from '@/lib/api';

interface AddToolDialogProps {
  open: boolean;
  onClose: () => void;
  onAdded: () => void;
}

export default function AddToolDialog({ open, onClose, onAdded }: AddToolDialogProps) {
  const [name, setName] = useState('');
  const [description, setDescription] = useState('');
  const [toolType, setToolType] = useState<'http' | 'cli'>('http');

  // HTTP config
  const [method, setMethod] = useState('POST');
  const [url, setUrl] = useState('');
  const [headers, setHeaders] = useState('{}');
  const [body, setBody] = useState('{}');

  // CLI config
  const [command, setCommand] = useState('');

  // Schema
  const [schema, setSchema] = useState('{\n  "type": "object",\n  "properties": {},\n  "required": []\n}');

  const [saving, setSaving] = useState(false);
  const [error, setError] = useState('');

  if (!open) return null;

  const handleSave = async () => {
    if (!name.trim()) {
      setError('Name is required');
      return;
    }

    setSaving(true);
    setError('');

    try {
      const id = `external:${name.toLowerCase().replace(/\s+/g, '_')}`;

      let config;
      if (toolType === 'http') {
        config = {
          type: 'http',
          method,
          url,
          headers: JSON.parse(headers),
          body: JSON.parse(body),
        };
      } else {
        config = {
          type: 'cli',
          command,
        };
      }

      let parsedSchema;
      try {
        parsedSchema = JSON.parse(schema);
      } catch {
        setError('Invalid JSON schema');
        setSaving(false);
        return;
      }

      await apiFetch('/tools', {
        method: 'POST',
        body: JSON.stringify({
          id,
          name: name.trim(),
          description: description.trim() || null,
          category: 'external',
          config,
          schema: parsedSchema,
        }),
      });

      onAdded();
      onClose();
      // Reset form
      setName('');
      setDescription('');
      setUrl('');
      setHeaders('{}');
      setBody('{}');
      setCommand('');
    } catch (e) {
      setError(String(e));
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50">
      <div className="bg-card border border-border rounded-xl w-full max-w-lg max-h-[90vh] overflow-y-auto p-6 space-y-4">
        <div className="flex items-center justify-between">
          <h2 className="text-lg font-semibold">Add External Tool</h2>
          <button onClick={onClose} className="p-1 hover:bg-muted rounded">
            <X className="w-5 h-5" />
          </button>
        </div>

        {error && (
          <div className="text-sm text-destructive bg-destructive/10 rounded-lg px-3 py-2">
            {error}
          </div>
        )}

        <div>
          <label className="block text-sm font-medium mb-1 text-muted-foreground">Name</label>
          <input
            type="text"
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder="e.g. Move to Album (Immich)"
            className="w-full px-3 py-2 rounded-lg border border-input bg-background text-foreground text-sm focus:outline-none focus:ring-2 focus:ring-ring"
          />
        </div>

        <div>
          <label className="block text-sm font-medium mb-1 text-muted-foreground">Description</label>
          <input
            type="text"
            value={description}
            onChange={(e) => setDescription(e.target.value)}
            placeholder="What this tool does..."
            className="w-full px-3 py-2 rounded-lg border border-input bg-background text-foreground text-sm focus:outline-none focus:ring-2 focus:ring-ring"
          />
        </div>

        <div>
          <label className="block text-sm font-medium mb-1 text-muted-foreground">Type</label>
          <select
            value={toolType}
            onChange={(e) => setToolType(e.target.value as 'http' | 'cli')}
            className="w-full px-3 py-2 rounded-lg border border-input bg-background text-foreground text-sm focus:outline-none focus:ring-2 focus:ring-ring"
          >
            <option value="http">HTTP API</option>
            <option value="cli">CLI Command</option>
          </select>
        </div>

        {toolType === 'http' ? (
          <>
            <div className="grid grid-cols-4 gap-2">
              <div>
                <label className="block text-sm font-medium mb-1 text-muted-foreground">Method</label>
                <select
                  value={method}
                  onChange={(e) => setMethod(e.target.value)}
                  className="w-full px-3 py-2 rounded-lg border border-input bg-background text-foreground text-sm focus:outline-none focus:ring-2 focus:ring-ring"
                >
                  <option>GET</option>
                  <option>POST</option>
                  <option>PUT</option>
                  <option>PATCH</option>
                  <option>DELETE</option>
                </select>
              </div>
              <div className="col-span-3">
                <label className="block text-sm font-medium mb-1 text-muted-foreground">URL (use {'{param}'} for placeholders)</label>
                <input
                  type="text"
                  value={url}
                  onChange={(e) => setUrl(e.target.value)}
                  placeholder="http://immich:3001/api/album/{album_id}/assets"
                  className="w-full px-3 py-2 rounded-lg border border-input bg-background text-foreground text-sm focus:outline-none focus:ring-2 focus:ring-ring"
                />
              </div>
            </div>
            <div>
              <label className="block text-sm font-medium mb-1 text-muted-foreground">Headers (JSON)</label>
              <textarea
                value={headers}
                onChange={(e) => setHeaders(e.target.value)}
                rows={3}
                className="w-full px-3 py-2 rounded-lg border border-input bg-background text-foreground text-sm font-mono focus:outline-none focus:ring-2 focus:ring-ring"
              />
            </div>
            <div>
              <label className="block text-sm font-medium mb-1 text-muted-foreground">Body Template (JSON, use {'{param}'} for placeholders)</label>
              <textarea
                value={body}
                onChange={(e) => setBody(e.target.value)}
                rows={3}
                className="w-full px-3 py-2 rounded-lg border border-input bg-background text-foreground text-sm font-mono focus:outline-none focus:ring-2 focus:ring-ring"
              />
            </div>
          </>
        ) : (
          <div>
            <label className="block text-sm font-medium mb-1 text-muted-foreground">Command Template (use {'{param}'} for placeholders)</label>
            <textarea
              value={command}
              onChange={(e) => setCommand(e.target.value)}
              rows={3}
              placeholder="immich upload --album {album_id} {file_path}"
              className="w-full px-3 py-2 rounded-lg border border-input bg-background text-foreground text-sm font-mono focus:outline-none focus:ring-2 focus:ring-ring"
            />
          </div>
        )}

        <div>
          <label className="block text-sm font-medium mb-1 text-muted-foreground">Parameters Schema (JSON Schema)</label>
          <textarea
            value={schema}
            onChange={(e) => setSchema(e.target.value)}
            rows={6}
            className="w-full px-3 py-2 rounded-lg border border-input bg-background text-foreground text-sm font-mono focus:outline-none focus:ring-2 focus:ring-ring"
          />
        </div>

        <div className="flex gap-2 justify-end">
          <button
            onClick={onClose}
            className="px-4 py-2 rounded-lg border border-input text-sm hover:bg-muted"
          >
            Cancel
          </button>
          <button
            onClick={handleSave}
            disabled={saving}
            className="px-4 py-2 rounded-lg bg-primary text-primary-foreground text-sm hover:opacity-90 disabled:opacity-50"
          >
            {saving ? 'Saving...' : 'Add Tool'}
          </button>
        </div>
      </div>
    </div>
  );
}
