import { createHashRouter } from 'react-router-dom';
import { ReleaseConsoleScreen } from '../screens/ReleaseConsoleScreen';
import { ArtefactsScreen } from '../screens/ArtefactsScreen';
import { ChannelsScreen } from '../screens/ChannelsScreen';
import { BundlesScreen } from '../screens/BundlesScreen';
import { FleetScreen } from '../screens/FleetScreen';
import { RollbackScreen } from '../screens/RollbackScreen';

/* Hash routing so the console serves from any static path (air-gap-first,
 * `base: './'`) with no server rewrites. Paths mirror NAV[bifrost] item hrefs. */
export const router = createHashRouter([
  { path: '/', element: <ReleaseConsoleScreen /> },
  { path: '/artefacts', element: <ArtefactsScreen /> },
  { path: '/channels', element: <ChannelsScreen /> },
  { path: '/bundles', element: <BundlesScreen /> },
  { path: '/fleet', element: <FleetScreen /> },
  { path: '/rollback', element: <RollbackScreen /> },
]);
