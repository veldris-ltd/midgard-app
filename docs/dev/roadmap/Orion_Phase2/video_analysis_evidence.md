# Verified observations from video OCR (ssstwitter_com_1785634510106.mp4)
# Method: 28 frames @ 1/4s + tesseract OCR. Video 112s, 720p. Audio present (music, no ASR available).
# Evidence = OCR text from specific frames. Conservative reading applied.

## Confirmed on-screen (high confidence — literal text)
- **"FMV/SIP Demo Video"** — title (frame 004). This is a Full-Motion Video / Sensor Integration Panel demo. Palantir Gotham-family UI (menu bar: File/Edit/View/Element/Map/Tools/Help; "Search this map").
- **Video Source / Video Source System** + **"Live Video"** layer (frame 004). Live FMV feed as a map layer.
- **PLATFORM** telemetry block (frames 005-007): Heading, Elevation, Roll, Pitch, relative Azimuth, coordinates (lat/long like 155807, coordinate strings). A sensor platform with full pose/attitude.
- **SENSOR** block with sensor model id **"FUR SS3 HD" / "FURSS3HDHDVIDEO"** (frames 005-007) — a FLIR/EO-IR sensor type identifier. Sensor metadata: Type, relative Azimuth, Elevation, Roll, Pitch.
- **"Operator Fury Woody"** / callsign-style operator + platform naming (frame 004).
- **Computer Vision Detection** (frames 010, 012) — automated CV object detection, with detected classes: **Airplane, Assault Vessel, Vehicle** (frames 010, 012). Detection has "Tracking" state.
- **"Target Board"** (frame 010, "SARA Board"/"Target Board") — a panel listing detections/tracks with scores.
- **Detection scoring / confidence** ("scores") on detections (frame 010).
- **Nomination flow** — detections can be nominated (candidate views, frame 009 "Test candidate views").
- **Asset Tasking Recommender** (frames 021-022):
  - Column headers: **Match (Effect), Time to Target, Distance, Priority** (frame 021).
  - **"Optimize Recommender"** with weightable factors: **Time on Station × Fuel × Munitions** (frame 021).
  - **"Show all metrics"** toggle (frame 021).
  - **"Top Match"** recommendation card (frame 022): asset **STRYKER1**, detection type "Computer Vision Detection", weapon **M2 .50 cal (75.0%)**, **2x Aimpoints**, Elevation/Speed/Heading (599 ft, 0 km/h, 000), **MUNITIONS AVAIL / ALLOC / SPENT** (M2 .50 cal 750 / 0 / 0), **Distance**, **Time to Target 4m 23s / 141.8 nm**.
  - **"LIVE (updated 0 seconds ago)"** vs **"Simulation"** toggle (frame 022).
  - **"Edit Asset"** action (frame 022).
- **Runway / Deliberate / Dynamic** targeting terms (frame 009: "Runway", "Deliberate", "Dynamic ... Position").
- Map with route/geometry tools, layer control, entity selection (frames 004-007).

## Interpretation (what capability each maps to)
1. FMV ingestion + live video layer geo-registered on the map -> FMV service
2. Platform + Sensor pose/telemetry panel (FLIR SS3 HD) -> Sensor Integration Panel (SIP) with full platform/sensor attitude model
3. Computer Vision Detection (Airplane/Vessel/Vehicle) with tracking + confidence -> CV detection + tracking layer
4. Target Board with nomination + scoring -> Target Board workflow
5. Asset Tasking Recommender with Match/Effect, Time-to-Target, Distance, Priority; Optimize weights (Time on Station/Fuel/Munitions); Top Match card with asset+munition+availability; LIVE vs Simulation -> Asset Tasking Recommender with effects-based optimisation + munitions accounting + simulation mode
6. Deliberate vs Dynamic targeting modes -> targeting workflow modes

## NEW capabilities beyond the prior Orion (screenshot-based) addendum
- **Computer Vision auto-detection & classification** (Airplane/Assault Vessel/Vehicle) with confidence scores — the prior addendum had FMV but NOT the CV auto-detection pipeline.
- **Platform+Sensor attitude/pose telemetry model** (heading/elevation/roll/pitch/azimuth, sensor model id) — richer than the prior SIP.
- **Effects-based tasking optimisation** — Match(Effect) scoring, weightable Optimize (Time on Station × Fuel × Munitions) — prior had ranked options but not weighted effects optimisation.
- **Munitions accounting** (AVAIL/ALLOC/SPENT per weapon) — new.
- **LIVE vs Simulation mode** for tasking — new; lets operators model a tasking before committing.
- **Deliberate vs Dynamic targeting modes** — new workflow distinction.
- **Time-to-Target / Distance computation** per asset-target pair — new.
