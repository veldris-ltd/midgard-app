/* The offline build (VITE_OFFLINE=1, the Mac app): the console never reaches the
 * internet. RADAR replays the frames recorded in the app (radar.ts), TRAFFIC shows the
 * recorded snapshot (traffic.ts), and the globe's close-up reads the recorded terrain and
 * satellite tiles (globe/detailPatch.ts; the bundled map outside them).
 * Unset (the dev server), everything works as before. A build-time constant, so the
 * bundler drops the internet code paths from the offline build. */
export const OFFLINE: boolean = import.meta.env.VITE_OFFLINE === '1';
