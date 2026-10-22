/* The offline build (VITE_OFFLINE=1, the Mac app): the console never reaches the
 * internet. The RADAR and TRAFFIC chips are hidden and their fetches never start, and the
 * globe keeps the bundled map close up instead of reading terrain and satellite tiles.
 * Unset (the dev server), everything works as before. A build-time constant, so the
 * bundler drops the internet code paths from the offline build. */
export const OFFLINE: boolean = import.meta.env.VITE_OFFLINE === '1';
