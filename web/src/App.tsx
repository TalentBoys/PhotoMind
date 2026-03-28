import { BrowserRouter, Routes, Route, NavLink } from 'react-router-dom';
import { Search, MessageSquare, Settings } from 'lucide-react';
import SearchPage from '@/pages/SearchPage';
import ChatPage from '@/pages/ChatPage';
import SettingsPage from '@/pages/SettingsPage';

function NavBar() {
  const linkClass = ({ isActive }: { isActive: boolean }) =>
    `flex items-center gap-2 px-4 py-2 rounded-lg text-sm font-medium transition-colors ${
      isActive ? 'bg-primary text-primary-foreground' : 'text-muted-foreground hover:bg-muted'
    }`;

  return (
    <nav className="border-b border-border px-4 h-14 flex items-center gap-2">
      <span className="font-bold text-lg mr-4 text-primary">PhotoMind</span>
      <NavLink to="/" className={linkClass} end>
        <Search className="w-4 h-4" /> Search
      </NavLink>
      <NavLink to="/chat" className={linkClass}>
        <MessageSquare className="w-4 h-4" /> Chat
      </NavLink>
      <NavLink to="/settings" className={linkClass}>
        <Settings className="w-4 h-4" /> Settings
      </NavLink>
    </nav>
  );
}

export default function App() {
  return (
    <BrowserRouter>
      <div className="min-h-screen bg-background text-foreground">
        <NavBar />
        <Routes>
          <Route path="/" element={<SearchPage />} />
          <Route path="/chat" element={<ChatPage />} />
          <Route path="/settings" element={<SettingsPage />} />
        </Routes>
      </div>
    </BrowserRouter>
  );
}
