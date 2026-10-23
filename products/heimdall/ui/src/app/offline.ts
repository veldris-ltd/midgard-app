/* The offline build (VITE_OFFLINE=1, the Mac app): the console never reaches the
 * internet. RADAR replays the frames recorded in the app (radar.ts), TRAFFIC shows the
 * recorded snapshot (traffic.ts), and the globe keeps the bundled map close up instead of
 * reading terrain and satellite tiles.
 * Unset (the dev server), everything works as before. A build-time constant, so the
 * bundler drops the internet code paths from the offline build. */
export const OFFLINE: boolean = import.meta.env.VITE_OFFLINE === '1';
