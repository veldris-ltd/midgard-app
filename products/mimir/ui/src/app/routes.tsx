import { createHashRouter } from 'react-router-dom';
import { AgentStudioScreen } from '../screens/AgentStudioScreen';
import { LogicBuilderScreen } from '../screens/LogicBuilderScreen';
import { SessionTracesScreen } from '../screens/SessionTracesScreen';
import { EvalsScreen } from '../screens/EvalsScreen';
import { GuardrailsScreen } from '../screens/GuardrailsScreen';
import { FeedbackScreen } from '../screens/FeedbackScreen';
import { ModelPlaneScreen } from '../screens/ModelPlaneScreen';

/* Hash routing so the console serves from any static path (air-gap-first,
 * `base: './'`) with no server rewrites. Paths mirror NAV[mimir] item hrefs. */
export const router = createHashRouter([
  { path: '/', element: <AgentStudioScreen /> },
  { path: '/logic', element: <LogicBuilderScreen /> },
  { path: '/sessions', element: <SessionTracesScreen /> },
  { path: '/evals', element: <EvalsScreen /> },
  { path: '/guardrails', element: <GuardrailsScreen /> },
  { path: '/feedback', element: <FeedbackScreen /> },
  { path: '/models', element: <ModelPlaneScreen /> },
]);
