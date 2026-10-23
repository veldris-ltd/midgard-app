/** The credit each camera source's terms ask for, shown with the camera's image or clip,
 *  from the camera id's prefix (heimdall/feeds-runner/feeds.py: JamCams_… TfL JamCams,
 *  BCCAM-… DriveBC, NZCAM-… NZTA, SGCAM-… LTA via data.gov.sg). Null for any other camera
 *  (your own cameras, a source added later): the card then says the media is the operator's. */
export function cameraCredit(id: string): string | null {
  if (/^JamCams_/i.test(id)) return 'POWERED BY TFL OPEN DATA';
  if (/^BCCAM/i.test(id)) return 'DRIVEBC · OPEN GOVERNMENT LICENCE – BRITISH COLUMBIA';
  if (/^NZCAM/i.test(id)) return 'NZ TRANSPORT AGENCY WAKA KOTAHI';
  if (/^SGCAM/i.test(id)) return 'LTA VIA DATA.GOV.SG · SINGAPORE OPEN DATA LICENCE';
  return null;
}
