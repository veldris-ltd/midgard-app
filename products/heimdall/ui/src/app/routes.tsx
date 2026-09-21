import { createHashRouter } from 'react-router-dom';
import { OperatingPictureScreen } from '../screens/OperatingPictureScreen';
import { FeedsScreen } from '../screens/FeedsScreen';
import { EntitiesScreen } from '../screens/EntitiesScreen';
import { PictureSyncScreen } from '../screens/PictureSyncScreen';
import { WorkflowsScreen } from '../screens/WorkflowsScreen';
import { DecisionLineageScreen } from '../screens/DecisionLineageScreen';

/* Hash routing so the console serves from any static path (air-gap-first,
 * `base: './'`) with no server rewrites. Paths mirror NAV[heimdall] item hrefs. */
export const router = createHashRouter([
  { path: '/', element: <OperatingPictureScreen /> },
  { path: '/feeds', element: <FeedsScreen /> },
  { path: '/entities', element: <EntitiesScreen /> },
  { path: '/sync', element: <PictureSyncScreen /> },
  { path: '/workflows', element: <WorkflowsScreen /> },
  { path: '/lineage', element: <DecisionLineageScreen /> },
]);
