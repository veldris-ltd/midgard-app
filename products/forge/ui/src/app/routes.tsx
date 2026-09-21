import { createHashRouter } from 'react-router-dom';
import { PipelineBuilderScreen } from '../screens/PipelineBuilderScreen';
import { ConnectorsScreen } from '../screens/ConnectorsScreen';
import { DatasetsScreen } from '../screens/DatasetsScreen';
import { DataHealthScreen } from '../screens/DataHealthScreen';
import { SchedulesScreen } from '../screens/SchedulesScreen';
import { TransformIdeScreen } from '../screens/TransformIdeScreen';
import { ObjectProjectionScreen } from '../screens/ObjectProjectionScreen';

/* Hash routing so the console serves from any static path (air-gap-first,
 * `base: './'`) with no server rewrites. Paths mirror NAV[forge] item hrefs. */
export const router = createHashRouter([
  { path: '/', element: <PipelineBuilderScreen /> },
  { path: '/connectors', element: <ConnectorsScreen /> },
  { path: '/datasets', element: <DatasetsScreen /> },
  { path: '/health', element: <DataHealthScreen /> },
  { path: '/schedules', element: <SchedulesScreen /> },
  { path: '/ide', element: <TransformIdeScreen /> },
  { path: '/projection', element: <ObjectProjectionScreen /> },
]);
