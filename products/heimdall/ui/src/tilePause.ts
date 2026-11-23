/* Pausing map tiles after camera moves driven by the tracks, in the `public` build.
 * [FR-H-1] [NFR-PRV-1]
 *
 * A public tile request names the area in view, so a view framed on the tracks would
 * tell the tile hosts where the tracks are. In a `public` build, therefore, a move the
 * tracks decide (the first view fitted to them, SECTOR NORTH; flying to a track; any
 * preset fitted to track positions) pauses tile loading, and only a move the user makes
 * that is not centred on a track resumes it: a drag, a zoom, a tilt or turn, or a
 * geographic preset such as UK & IRELAND or SNOWDONIA. A `mirror` build never pauses:
 * its requests stay inside the enclosure. Pure. */

/** What moved the camera. */
export type CameraMove = 'track' | 'user';

export const TILES_PAUSED_NOTICE = 'MAP TILES PAUSED · PAN AWAY OR CHOOSE A REGION';

/** Whether tile loading is paused after `move`, when the build pauses on track moves. */
export function pausedAfter(move: CameraMove, pauseOnTrackMoves: boolean): boolean {
  return pauseOnTrackMoves && move === 'track';
}

/** Flying to a region: a track move when it is framed on the tracks, else the user's own. */
export function regionMove(region: { fittedToTracks?: boolean }): CameraMove {
  return region.fittedToTracks ? 'track' : 'user';
}
