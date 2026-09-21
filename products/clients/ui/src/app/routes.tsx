import { createHashRouter } from 'react-router-dom';
import { WebPage, MobilePage, DesktopPage } from '../App';

/* Hash routing so the gallery serves from any static path (air-gap-first,
 * `base: './'`) with no server rewrites. Each route is a device target; the
 * shared `ClientApp` core renders inside every one. */
export const router = createHashRouter([
  { path: '/', element: <WebPage /> },
  { path: '/mobile', element: <MobilePage /> },
  { path: '/desktop', element: <DesktopPage /> },
]);
