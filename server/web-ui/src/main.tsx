import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';

import { ApplicationShell } from './application-shell';
import './application.css';

const container = document.getElementById('weavelit-root');
if (container === null) {
  throw new Error('the Weavelit Web UI root element is missing');
}

createRoot(container).render(
  <StrictMode>
    <ApplicationShell />
  </StrictMode>,
);
