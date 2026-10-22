/* The Vite env values ui/shared reads, for its own typecheck (ui/shared has no Vite
 * dependency; the consoles get the same types from vite/client). Read them as
 * `import.meta.env.VITE_…` so Vite inlines the single value, not the env object. */
interface ImportMetaEnv {
  readonly VITE_GATEWAY_URL?: string;
}
interface ImportMeta {
  readonly env: ImportMetaEnv;
}
