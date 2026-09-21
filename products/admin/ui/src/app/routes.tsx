import { createHashRouter } from 'react-router-dom';
import { OverviewScreen } from '../screens/OverviewScreen';
import { ApprovalsScreen } from '../screens/ApprovalsScreen';
import { IamScreen } from '../screens/IamScreen';
import { ConfigScreen } from '../screens/ConfigScreen';
import { RegistryScreen } from '../screens/RegistryScreen';
import { LedgerScreen } from '../screens/LedgerScreen';
import { BreakGlassScreen } from '../screens/BreakGlassScreen';
import { CopilotScreen } from '../screens/CopilotScreen';

/* Hash routing so the console serves from any static path (air-gap-first,
 * `base: './'`) with no server rewrites. Paths mirror NAV[admin] item hrefs. */
export const router = createHashRouter([
  { path: '/', element: <OverviewScreen /> },
  { path: '/approvals', element: <ApprovalsScreen /> },
  { path: '/iam', element: <IamScreen /> },
  { path: '/config', element: <ConfigScreen /> },
  { path: '/registry', element: <RegistryScreen /> },
  { path: '/ledger', element: <LedgerScreen /> },
  { path: '/breakglass', element: <BreakGlassScreen /> },
  { path: '/copilot', element: <CopilotScreen /> },
]);
