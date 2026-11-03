// Lets `node --test` run the SDK's TypeScript sources as they are: Node strips the
// types, and this resolves the extensionless relative imports the bundler accepts
// (`'../mock/MockGateway'`) to the `.ts` file beside them. Test-only; nothing ships.
// [VLD-ANB-MIDGARD-0021]
import { registerHooks } from 'node:module';

registerHooks({
  resolve(specifier, context, nextResolve) {
    const relative = specifier.startsWith('./') || specifier.startsWith('../');
    if (relative && !/\.[cm]?[jt]sx?$/.test(specifier)) {
      try {
        return nextResolve(`${specifier}.ts`, context);
      } catch {
        /* not a .ts file: fall through to the default resolution */
      }
    }
    return nextResolve(specifier, context);
  },
});
