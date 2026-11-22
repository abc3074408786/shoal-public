import {installDemo} from './demo';
import './app.css';

const query = new URLSearchParams(location.search);
installDemo(query);
if (query.get('view') === 'plugins') void import('./plugins');
else void import('../src/main');
