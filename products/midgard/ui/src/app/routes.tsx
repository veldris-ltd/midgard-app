import { createHashRouter } from 'react-router-dom';
import { ObjectFabricScreen } from '../screens/ObjectFabricScreen';
import { SearchAroundScreen } from '../screens/SearchAroundScreen';
import { PoliciesScreen } from '../screens/PoliciesScreen';
import { AuditScreen } from '../screens/AuditScreen';
import { IdentityScreen } from '../screens/IdentityScreen';
import { KeysScreen } from '../screens/KeysScreen';
import { ComputeScreen } from '../screens/ComputeScreen';
import { SdkScreen } from '../screens/SdkScreen';
import { ObservabilityScreen } from '../screens/ObservabilityScreen';
import { ResilienceScreen } from '../screens/ResilienceScreen';

/* Hash routing so the console serves from any static path (air-gap-first,
 * `base: './'`) with no server rewrites. Paths mirror NAV[midgard] item hrefs. */
export const router = createHashRouter([
  { path: '/', element: <ObjectFabricScreen /> },
  { path: '/search', element: <SearchAroundScreen /> },
  { path: '/policies', element: <PoliciesScreen /> },
  { path: '/audit', element: <AuditScreen /> },
  { path: '/identity', element: <IdentityScreen /> },
  { path: '/kms', element: <KeysScreen /> },
  { path: '/compute', element: <ComputeScreen /> },
  { path: '/sdk', element: <SdkScreen /> },
  { path: '/observability', element: <ObservabilityScreen /> },
  { path: '/resilience', element: <ResilienceScreen /> },
]);
