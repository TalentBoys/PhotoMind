import { useEffect, useState, useCallback, useRef } from 'react';
import { X, Download, ChevronLeft, ChevronRight, MapPin, ZoomIn, ZoomOut, RotateCcw } from 'lucide-react';

export interface PhotoItem {
  id: number;
  file_name: string;
}

interface PhotoLightboxProps {
  photos: PhotoItem[];
  openIndex: number | null;
  onClose: () => void;
}

interface PhotoInfo {
  file_name: string;
  file_path: string;
  file_size: number | null;
  width: number | null;
  height: number | null;
  format: string | null;
  taken_at: string | null;
  latitude: number | null;
  longitude: number | null;
}

const infoCache = new Map<number, PhotoInfo>();

async function fetchPhotoInfo(id: number): Promise<PhotoInfo | null> {
  if (infoCache.has(id)) return infoCache.get(id)!;
  try {
    const res = await fetch(`/api/photos/${id}`);
    if (!res.ok) return null;
    const info: PhotoInfo = await res.json();
    infoCache.set(id, info);
    return info;
  } catch { return null; }
}

function formatSize(bytes: number | null): string {
  if (bytes == null) return '';
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

function formatDate(dateStr: string | null): string {
  if (!dateStr) return '';
  try {
    return new Date(dateStr).toLocaleDateString(undefined, {
      year: 'numeric', month: 'short', day: 'numeric',
      hour: '2-digit', minute: '2-digit',
    });
  } catch { return dateStr; }
}

export default function PhotoLightbox({ photos, openIndex, onClose }: PhotoLightboxProps) {
  const [currentIndex, setCurrentIndex] = useState(0);
  const [info, setInfo] = useState<PhotoInfo | null>(null);
  const [imageLoaded, setImageLoaded] = useState(false);
  const [scale, setScale] = useState(1);
  const [translate, setTranslate] = useState({ x: 0, y: 0 });
  const [dragging, setDragging] = useState(false);
  const dragStart = useRef({ x: 0, y: 0, tx: 0, ty: 0 });
  const containerRef = useRef<HTMLDivElement>(null);

  const isOpen = openIndex != null && photos.length > 0;
  const photo = isOpen ? photos[currentIndex] : null;

  // Sync index when openIndex prop changes
  useEffect(() => {
    if (openIndex != null && openIndex >= 0 && openIndex < photos.length) {
      setCurrentIndex(openIndex);
      setScale(1);
      setTranslate({ x: 0, y: 0 });
    }
  }, [openIndex, photos.length]);

  // Fetch info when current photo changes
  useEffect(() => {
    if (!photo) { setInfo(null); return; }
    setInfo(infoCache.get(photo.id) ?? null);
    setImageLoaded(false);
    fetchPhotoInfo(photo.id).then((i) => { if (i) setInfo(i); });
  }, [photo]);

  const goTo = useCallback((idx: number) => {
    setCurrentIndex(idx);
    setScale(1);
    setTranslate({ x: 0, y: 0 });
  }, []);

  const goPrev = useCallback(() => {
    if (photos.length <= 1) return;
    goTo((currentIndex - 1 + photos.length) % photos.length);
  }, [currentIndex, photos.length, goTo]);

  const goNext = useCallback(() => {
    if (photos.length <= 1) return;
    goTo((currentIndex + 1) % photos.length);
  }, [currentIndex, photos.length, goTo]);

  const zoomIn = useCallback(() => setScale((s) => Math.min(s * 1.5, 8)), []);
  const zoomOut = useCallback(() => {
    setScale((s) => {
      const next = Math.max(s / 1.5, 1);
      if (next === 1) setTranslate({ x: 0, y: 0 });
      return next;
    });
  }, []);
  const resetZoom = useCallback(() => { setScale(1); setTranslate({ x: 0, y: 0 }); }, []);

  // Keyboard navigation
  useEffect(() => {
    if (!isOpen) return;
    const handleKey = (e: KeyboardEvent) => {
      switch (e.key) {
        case 'Escape': onClose(); break;
        case 'ArrowLeft': goPrev(); break;
        case 'ArrowRight': goNext(); break;
        case '+': case '=': zoomIn(); break;
        case '-': zoomOut(); break;
        case '0': resetZoom(); break;
      }
    };
    window.addEventListener('keydown', handleKey);
    return () => window.removeEventListener('keydown', handleKey);
  }, [isOpen, onClose, goPrev, goNext, zoomIn, zoomOut, resetZoom]);

  // Lock body scroll
  useEffect(() => {
    if (isOpen) {
      document.body.style.overflow = 'hidden';
      return () => { document.body.style.overflow = ''; };
    }
  }, [isOpen]);

  // Mouse wheel zoom
  const handleWheel = useCallback((e: React.WheelEvent) => {
    e.preventDefault();
    if (e.deltaY < 0) zoomIn();
    else zoomOut();
  }, [zoomIn, zoomOut]);

  // Pan (drag when zoomed)
  const handleMouseDown = useCallback((e: React.MouseEvent) => {
    if (scale <= 1) return;
    e.preventDefault();
    setDragging(true);
    dragStart.current = { x: e.clientX, y: e.clientY, tx: translate.x, ty: translate.y };
  }, [scale, translate]);

  const handleMouseMove = useCallback((e: React.MouseEvent) => {
    if (!dragging) return;
    const dx = e.clientX - dragStart.current.x;
    const dy = e.clientY - dragStart.current.y;
    setTranslate({ x: dragStart.current.tx + dx, y: dragStart.current.ty + dy });
  }, [dragging]);

  const handleMouseUp = useCallback(() => { setDragging(false); }, []);

  if (!isOpen || !photo) return null;

  return (
    <div className="fixed inset-0 z-50 flex flex-col bg-black/90" onClick={(e) => { if (e.target === e.currentTarget) onClose(); }}>
      {/* Top bar */}
      <div className="flex items-center justify-between px-4 py-3 bg-black/60 text-white shrink-0">
        <span className="text-sm font-medium truncate max-w-xs">{photo.file_name}</span>
        <div className="flex items-center gap-1">
          <button onClick={zoomIn} className="p-2 rounded-lg hover:bg-white/10 transition" title="Zoom in (+)"><ZoomIn className="w-5 h-5" /></button>
          <button onClick={zoomOut} className="p-2 rounded-lg hover:bg-white/10 transition" title="Zoom out (-)"><ZoomOut className="w-5 h-5" /></button>
          <button onClick={resetZoom} className="p-2 rounded-lg hover:bg-white/10 transition" title="Reset zoom (0)"><RotateCcw className="w-5 h-5" /></button>
          <span className="text-xs text-white/50 w-12 text-center">{Math.round(scale * 100)}%</span>
          <a
            href={`/api/photos/${photo.id}/file`}
            download
            className="p-2 rounded-lg hover:bg-white/10 transition"
            title="Download original"
          >
            <Download className="w-5 h-5" />
          </a>
          <button onClick={onClose} className="p-2 rounded-lg hover:bg-white/10 transition" title="Close (Esc)"><X className="w-5 h-5" /></button>
        </div>
      </div>

      {/* Main area */}
      <div className="flex flex-1 min-h-0">
        {/* Image area */}
        <div
          ref={containerRef}
          className="flex-1 relative flex items-center justify-center overflow-hidden select-none"
          onWheel={handleWheel}
          onMouseDown={handleMouseDown}
          onMouseMove={handleMouseMove}
          onMouseUp={handleMouseUp}
          onMouseLeave={handleMouseUp}
          style={{ cursor: scale > 1 ? (dragging ? 'grabbing' : 'grab') : 'default' }}
        >
          {/* Nav arrows */}
          {photos.length > 1 && (
            <>
              <button
                onClick={(e) => { e.stopPropagation(); goPrev(); }}
                className="absolute left-3 top-1/2 -translate-y-1/2 z-10 p-2 rounded-full bg-black/50 text-white hover:bg-black/70 transition"
              >
                <ChevronLeft className="w-6 h-6" />
              </button>
              <button
                onClick={(e) => { e.stopPropagation(); goNext(); }}
                className="absolute right-3 top-1/2 -translate-y-1/2 z-10 p-2 rounded-full bg-black/50 text-white hover:bg-black/70 transition"
              >
                <ChevronRight className="w-6 h-6" />
              </button>
            </>
          )}

          {/* Loading spinner */}
          {!imageLoaded && (
            <div className="absolute inset-0 flex items-center justify-center">
              <div className="w-10 h-10 border-4 border-white/20 border-t-white rounded-full animate-spin" />
            </div>
          )}

          {/* Photo */}
          <img
            key={photo.id}
            src={`/api/photos/${photo.id}/preview`}
            alt={photo.file_name}
            className="max-w-full max-h-full object-contain transition-opacity duration-200"
            style={{
              opacity: imageLoaded ? 1 : 0,
              transform: `translate(${translate.x}px, ${translate.y}px) scale(${scale})`,
              transformOrigin: 'center center',
            }}
            onLoad={() => setImageLoaded(true)}
            draggable={false}
          />

          {/* Counter */}
          {photos.length > 1 && (
            <div className="absolute bottom-3 left-1/2 -translate-x-1/2 text-white/60 text-sm bg-black/50 px-3 py-1 rounded-full">
              {currentIndex + 1} / {photos.length}
            </div>
          )}
        </div>

        {/* Info sidebar */}
        <div className="w-72 bg-black/70 text-white/90 p-4 overflow-y-auto shrink-0 border-l border-white/10 hidden md:block">
          <h3 className="text-sm font-semibold mb-3 text-white">Photo Info</h3>
          {info ? (
            <div className="space-y-2.5 text-sm">
              <InfoRow label="File" value={info.file_name} />
              {info.format && <InfoRow label="Format" value={info.format.toUpperCase()} />}
              {info.width && info.height && <InfoRow label="Dimensions" value={`${info.width} × ${info.height}`} />}
              {info.file_size != null && <InfoRow label="Size" value={formatSize(info.file_size)} />}
              {info.taken_at && <InfoRow label="Taken" value={formatDate(info.taken_at)} />}
              {info.latitude != null && info.longitude != null && (
                <div>
                  <p className="text-white/50 text-xs mb-1">Location</p>
                  <a
                    href={`https://www.google.com/maps?q=${info.latitude.toFixed(6)},${info.longitude.toFixed(6)}`}
                    target="_blank"
                    rel="noopener noreferrer"
                    className="flex items-center gap-1 text-blue-300 hover:text-blue-200 underline text-xs"
                  >
                    <MapPin className="w-3.5 h-3.5" />
                    {info.latitude.toFixed(6)}, {info.longitude.toFixed(6)}
                  </a>
                </div>
              )}
              <div>
                <p className="text-white/50 text-xs mb-1">Path</p>
                <p className="text-xs text-white/40 break-all">{info.file_path}</p>
              </div>
            </div>
          ) : (
            <p className="text-white/40 text-sm">Loading...</p>
          )}
        </div>
      </div>

      {/* Thumbnail strip */}
      {photos.length > 1 && (
        <div className="bg-black/70 border-t border-white/10 px-4 py-2 shrink-0">
          <div className="flex gap-2 overflow-x-auto justify-center">
            {photos.map((p, idx) => (
              <button
                key={p.id}
                onClick={() => goTo(idx)}
                className={`w-14 h-14 rounded-md overflow-hidden border-2 shrink-0 transition ${
                  idx === currentIndex ? 'border-white' : 'border-transparent opacity-50 hover:opacity-80'
                }`}
              >
                <img
                  src={`/api/photos/${p.id}/thumbnail`}
                  alt={p.file_name}
                  className="w-full h-full object-cover"
                  loading="lazy"
                />
              </button>
            ))}
          </div>
        </div>
      )}
    </div>
  );
}

function InfoRow({ label, value }: { label: string; value: string }) {
  return (
    <div>
      <p className="text-white/50 text-xs">{label}</p>
      <p className="text-white/90">{value}</p>
    </div>
  );
}
