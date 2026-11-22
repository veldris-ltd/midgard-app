/* The map tiles switch, `VITE_HEIMDALL_TILES`: on unless set to off, false, 0 or no
 * (any case, surrounding space ignored). vite.config.ts turns it into the build-time
 * constant `__HEIMDALL_TILES__`. Pure. [FR-H-1] */
export function tilesEnabled(value: string | undefined): boolean {
  return !['off', 'false', '0', 'no'].includes((value ?? '').trim().toLowerCase());
}
