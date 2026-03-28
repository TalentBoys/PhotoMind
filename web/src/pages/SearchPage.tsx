import { useState, type FormEvent } from 'react';
import { Search, ImagePlus } from 'lucide-react';
import PhotoLightbox from '@/components/PhotoLightbox';

interface SearchResult {
  id: number;
  file_path: string;
  file_name: string;
  score: number;
  width?: number;
  height?: number;
  format?: string;
  taken_at?: string;
  file_size?: number;
}

const PAGE_SIZE = 3;
const FETCH_LIMIT = 30;

export default function SearchPage() {
  const [query, setQuery] = useState('');
  const [allResults, setAllResults] = useState<SearchResult[]>([]);
  const [visibleCount, setVisibleCount] = useState(PAGE_SIZE);
  const [loading, setLoading] = useState(false);
  const [searched, setSearched] = useState(false);
  const [lightboxIndex, setLightboxIndex] = useState<number | null>(null);

  const results = allResults.slice(0, visibleCount);
  const hasMore = visibleCount < allResults.length;

  const handleSearch = async (e: FormEvent) => {
    e.preventDefault();
    if (!query.trim()) return;
    setLoading(true);
    setSearched(true);
    setVisibleCount(PAGE_SIZE);
    try {
      const res = await fetch('/api/search', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ query: query.trim(), limit: FETCH_LIMIT }),
      });
      if (res.ok) {
        const data = await res.json();
        setAllResults(data.results ?? []);
      }
    } finally {
      setLoading(false);
    }
  };

  const handleImageSearch = async (file: File) => {
    setLoading(true);
    setSearched(true);
    setVisibleCount(PAGE_SIZE);
    const formData = new FormData();
    formData.append('image', file);
    formData.append('limit', String(FETCH_LIMIT));
    try {
      const res = await fetch('/api/search/image', {
        method: 'POST',
        body: formData,
      });
      if (res.ok) {
        const data = await res.json();
        setAllResults(data.results ?? []);
      }
    } finally {
      setLoading(false);
    }
  };

  const photos = results.map((r) => ({ id: r.id, file_name: r.file_name }));

  return (
    <div className="flex flex-col items-center px-4 pt-20">
      <h1 className="text-4xl font-bold mb-2 text-primary">PhotoMind</h1>
      <p className="text-muted-foreground mb-8">AI-powered photo search</p>

      <form onSubmit={handleSearch} className="w-full max-w-2xl flex gap-2 mb-8">
        <div className="relative flex-1">
          <Search className="absolute left-3 top-1/2 -translate-y-1/2 w-5 h-5 text-muted-foreground" />
          <input
            type="text"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Describe the photo you're looking for..."
            className="w-full pl-10 pr-4 py-3 rounded-xl border border-input bg-card text-foreground focus:outline-none focus:ring-2 focus:ring-ring"
          />
        </div>
        <button
          type="submit"
          disabled={loading}
          className="px-6 py-3 rounded-xl bg-primary text-primary-foreground font-medium hover:opacity-90 disabled:opacity-50"
        >
          Search
        </button>
        <label className="px-3 py-3 rounded-xl border border-input bg-card hover:bg-muted cursor-pointer flex items-center">
          <ImagePlus className="w-5 h-5 text-muted-foreground" />
          <input
            type="file"
            accept="image/*"
            className="hidden"
            onChange={(e) => {
              const file = e.target.files?.[0];
              if (file) handleImageSearch(file);
            }}
          />
        </label>
      </form>

      {loading && <p className="text-muted-foreground">Searching...</p>}

      {!loading && searched && results.length === 0 && (
        <p className="text-muted-foreground">No results found.</p>
      )}

      {results.length > 0 && (
        <div className="w-full max-w-4xl grid grid-cols-3 gap-4">
          {results.map((r, idx) => (
            <div
              key={r.id}
              className="group relative aspect-square rounded-lg overflow-hidden border border-border bg-muted cursor-pointer"
              onClick={() => setLightboxIndex(idx)}
            >
              <img
                src={`/api/photos/${r.id}/thumbnail`}
                alt={r.file_name}
                className="w-full h-full object-cover"
                loading="lazy"
              />
              <div className="absolute inset-0 bg-black/60 opacity-0 group-hover:opacity-100 transition-opacity flex flex-col justify-end p-2 text-white text-xs">
                <p className="font-medium truncate">{r.file_name}</p>
                <p className="opacity-75 truncate">{r.file_path}</p>
                {r.score != null && (
                  <p className="opacity-75">Score: {(r.score * 100).toFixed(1)}%</p>
                )}
              </div>
            </div>
          ))}
        </div>
      )}

      {hasMore && !loading && (
        <button
          onClick={() => setVisibleCount((c) => c + PAGE_SIZE)}
          className="mt-6 px-6 py-2 rounded-xl border border-input bg-card text-foreground hover:bg-muted"
        >
          Show More
        </button>
      )}

      {!hasMore && allResults.length > 0 && visibleCount >= allResults.length && !loading && (
        <p className="mt-6 text-sm text-muted-foreground">
          No more results. Try a different description to continue searching.
        </p>
      )}

      <PhotoLightbox photos={photos} openIndex={lightboxIndex} onClose={() => setLightboxIndex(null)} />
    </div>
  );
}
