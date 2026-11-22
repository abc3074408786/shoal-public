import {useState} from 'react';
import {createRoot} from 'react-dom/client';
import AppSettings from '../src/AppSettings';
import {AppearanceProvider} from '../src/Appearance';
import {ConfirmationProvider} from '../src/ConfirmAction';
import PluginsPage from '../src/PluginsPage';
import {TooltipProvider} from '../src/ui';
import '../src/style.css';
import '../src/theme.css';
import '../src/orca-shell.css';
import '../src/motion.css';

const home = () => location.assign('./app.html');
function PrototypePlugins() {
  const [settings, setSettings] = useState(false);
  return settings ? <AppSettings onClose={() => setSettings(false)}/> :
    <PluginsPage onBack={home} onSettings={() => setSettings(true)}/>;
}
createRoot(document.getElementById('root')!).render(
  <AppearanceProvider><TooltipProvider><ConfirmationProvider>
    <PrototypePlugins/>
  </ConfirmationProvider></TooltipProvider></AppearanceProvider>,
);
